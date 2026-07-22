use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
};

use derive_more::Display;
use node_semver::{Identifier, Range, Version};

use crate::{
    error::VersioningError,
    intents::{ChangeIntent, IntentBumpType},
    ledger::{Ledger, PackageConsumption, build_consumption_index, normalize_project_dir},
    settings::{ReleaseBumpType, VersioningSettings},
};

/// One workspace project as the release-plan assembler sees it: the manifest
/// fields the plan depends on, extracted by the CLI layer.
#[derive(Debug, Clone)]
pub struct WorkspaceProject {
    pub root_dir: PathBuf,
    pub name: Option<String>,
    pub version: Option<String>,
    /// The entries of the manifest's production dependency fields
    /// (`dependencies`, `optionalDependencies`, `peerDependencies`).
    /// devDependencies never propagate — they are not part of the published
    /// artifact.
    pub prod_dependencies: Vec<ManifestDependency>,
}

#[derive(Debug, Clone)]
pub struct ManifestDependency {
    pub field: DependencyField,
    pub alias: String,
    pub spec: String,
}

#[derive(Debug, Display, Clone, Copy, PartialEq, Eq)]
pub enum DependencyField {
    #[display("dependencies")]
    Dependencies,
    #[display("optionalDependencies")]
    OptionalDependencies,
    #[display("peerDependencies")]
    PeerDependencies,
}

/// Causes are reported sorted by name, matching the TypeScript plan output:
/// `dependencies < epic < fixed < intent`.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReleaseCause {
    #[display("dependencies")]
    Dependencies,
    #[display("epic")]
    Epic,
    #[display("fixed")]
    Fixed,
    #[display("intent")]
    Intent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyUpdate {
    pub name: String,
    pub new_version: String,
}

#[derive(Debug, Clone)]
pub struct PlannedRelease {
    pub name: String,
    /// Workspace-relative project directory — the engine's unit of identity.
    pub dir: String,
    pub root_dir: PathBuf,
    pub current_version: String,
    pub new_version: String,
    pub bump_type: ReleaseBumpType,
    /// The intent files this release consumes for this package: the pending
    /// ones, plus — when the release graduates the package off a lane — the
    /// ones the ledger recorded against the lane's prerelease versions.
    pub intents: Vec<ChangeIntent>,
    pub dependency_updates: Vec<DependencyUpdate>,
    pub causes: Vec<ReleaseCause>,
}

#[derive(Debug, Default, Clone)]
pub struct ReleasePlan {
    pub releases: Vec<PlannedRelease>,
}

#[derive(Debug, Default, Clone)]
pub struct AssembleReleasePlanOptions {
    /// Workspace-relative directories of the projects selected with
    /// --filter. The plan is narrowed to the selected packages' portion of
    /// the pending work, expanded with their fixed-group companions and
    /// range-invalidated dependents.
    pub filter: Option<HashSet<String>>,
    /// When set, every planned release gets the version `0.0.0-<suffix>`
    /// instead of the computed one, matching snapshot releases.
    pub snapshot_suffix: Option<String>,
    /// Enforce that every internal production dependency uses the
    /// `workspace:` protocol — a prerequisite for actually releasing. The
    /// release path (`pnpm version -r`) sets this; read-only callers
    /// (`pnpm change status`) leave it off so a diagnostic never fails on an
    /// unmigrated dependency.
    pub enforce_workspace_protocol: bool,
    /// Directories whose current manifest version the registry does not have.
    /// Their first release publishes that version verbatim, so the pending
    /// change intents bump it only from the next release. Resolved by the CLI's
    /// registry probe. Fixed-group sharing and epic band re-basing still
    /// override it, since a package cannot opt out of those workspace-wide
    /// version rules.
    pub unpublished_dirs: HashSet<String>,
}

/// Whether a package reference is a workspace-relative directory path rather
/// than a package name — the additive extension to the changesets format,
/// needed only when workspace projects share a published name.
#[must_use]
pub fn is_dir_ref(reference: &str) -> bool {
    reference.starts_with("./")
}

/// The workspace-relative directory of a project, in canonical spelling.
#[must_use]
pub fn to_project_dir(workspace_dir: &Path, root_dir: &Path) -> String {
    let relative =
        pathdiff::diff_paths(root_dir, workspace_dir).unwrap_or_else(|| root_dir.to_path_buf());
    normalize_project_dir(&relative.to_string_lossy())
}

/// Resolves package references — bare names, or `./`-prefixed
/// workspace-relative directories — against the workspace. Names are
/// aliases: one that matches several projects cannot identify any of them
/// and callers must treat it as an error, never a silent pick.
pub struct ProjectRefIndex {
    dirs: HashSet<String>,
    dirs_by_name: HashMap<String, Vec<String>>,
}

impl ProjectRefIndex {
    /// The directories a reference resolves to: empty when unknown, two or
    /// more when the name is ambiguous.
    #[must_use]
    pub fn ref_to_dirs(&self, reference: &str) -> Vec<String> {
        if is_dir_ref(reference) {
            let dir = normalize_project_dir(reference);
            return if self.dirs.contains(&dir) { vec![dir] } else { Vec::new() };
        }
        self.dirs_by_name.get(reference).cloned().unwrap_or_default()
    }

    #[must_use]
    pub fn name_to_dirs(&self, name: &str) -> Vec<String> {
        self.dirs_by_name.get(name).cloned().unwrap_or_default()
    }
}

#[must_use]
pub fn index_project_refs(projects: &[WorkspaceProject], workspace_dir: &Path) -> ProjectRefIndex {
    let mut dirs = HashSet::new();
    let mut dirs_by_name: HashMap<String, Vec<String>> = HashMap::new();
    for project in projects {
        let dir = to_project_dir(workspace_dir, &project.root_dir);
        dirs.insert(dir.clone());
        if let Some(name) = &project.name {
            dirs_by_name.entry(name.clone()).or_default().push(dir);
        }
    }
    ProjectRefIndex { dirs, dirs_by_name }
}

pub fn assemble_release_plan(
    projects: &[WorkspaceProject],
    workspace_dir: &Path,
    intents: &[ChangeIntent],
    ledger: &Ledger,
    versioning: Option<&VersioningSettings>,
    opts: &AssembleReleasePlanOptions,
) -> Result<ReleasePlan, VersioningError> {
    let refs = index_project_refs(projects, workspace_dir);
    let participants = collect_participants(projects, workspace_dir, &refs, versioning)?;
    let lanes_by_dir = resolve_lanes(&refs, versioning)?;
    let fixed_groups = resolve_fixed_groups(&refs, &participants, versioning)?;
    validate_fixed_group_lanes(&fixed_groups, &lanes_by_dir, versioning)?;
    let epics = resolve_epics(&refs, &participants, versioning)?;
    validate_epics(&epics, &fixed_groups)?;
    let intent_bumps = resolve_intents(intents, &refs, &participants)?;
    if opts.enforce_workspace_protocol {
        assert_internal_deps_use_workspace_protocol(&participants)?;
    }
    let consumption = build_consumption_index(ledger, |name| refs.name_to_dirs(name))?;

    let ctx = AssembleContext {
        participants: &participants,
        lanes_by_dir: &lanes_by_dir,
        fixed_groups: &fixed_groups,
        epics: &epics,
        intent_bumps: &intent_bumps,
        consumption: &consumption,
        intents,
        versioning,
        opts,
    };
    let mut selection = opts.filter.clone();
    loop {
        let plan = assemble(&ctx, selection.as_ref())?;
        let Some(selected) = &mut selection else {
            return Ok(plan);
        };
        let before = selected.len();
        selected.extend(plan.releases.iter().map(|release| release.dir.clone()));
        if selected.len() == before {
            return Ok(plan);
        }
    }
}

struct Participant<'a> {
    name: &'a str,
    dir: String,
    root_dir: &'a Path,
    current_version: &'a str,
    internal_deps: Vec<InternalDep<'a>>,
}

struct InternalDep<'a> {
    target_dir: String,
    target_name: String,
    field: DependencyField,
    alias: &'a str,
    spec: &'a str,
}

struct BumpState {
    bump_type: ReleaseBumpType,
    causes: BTreeSet<ReleaseCause>,
    dependency_updates: BTreeMap<String, String>,
}

/// An epic resolved against the workspace: the lead's directory and the
/// directories of its member packages. The lead is never a member of its own
/// band.
struct ResolvedEpic {
    lead_ref: String,
    lead_dir: String,
    member_dirs: HashSet<String>,
}

struct AssembleContext<'a> {
    participants: &'a BTreeMap<String, Participant<'a>>,
    lanes_by_dir: &'a BTreeMap<String, String>,
    fixed_groups: &'a [Vec<String>],
    epics: &'a [ResolvedEpic],
    /// Per intent id: the participant dirs it releases and their bump types.
    intent_bumps: &'a HashMap<String, BTreeMap<String, IntentBumpType>>,
    consumption: &'a HashMap<String, PackageConsumption>,
    intents: &'a [ChangeIntent],
    versioning: Option<&'a VersioningSettings>,
    opts: &'a AssembleReleasePlanOptions,
}

impl AssembleContext<'_> {
    fn intent_bump_for(&self, intent: &ChangeIntent, dir: &str) -> Option<IntentBumpType> {
        self.intent_bumps.get(&intent.id).and_then(|by_dir| by_dir.get(dir)).copied()
    }
}

fn assemble(
    ctx: &AssembleContext<'_>,
    selection: Option<&HashSet<String>>,
) -> Result<ReleasePlan, VersioningError> {
    let participants = ctx.participants;
    let pending_by_dir = collect_pending_intents(ctx);
    let lane_consumed_by_dir = collect_lane_consumed_intents(ctx);

    let mut state: BTreeMap<String, BumpState> = BTreeMap::new();

    for (dir, pending) in &pending_by_dir {
        if selection.is_some_and(|selected| !selected.contains(dir)) {
            continue;
        }
        if let Some(direct) =
            max_bump_type(pending.iter().filter_map(|intent| ctx.intent_bump_for(intent, dir)))
        {
            bump_at_least(&mut state, dir, direct, ReleaseCause::Intent);
        }
    }

    // A package that left its lane releases the accumulated stable version
    // even when no new intents are pending.
    for (dir, lane_consumed) in &lane_consumed_by_dir {
        if selection.is_some_and(|selected| !selected.contains(dir)) {
            continue;
        }
        if ctx.lanes_by_dir.contains_key(dir) {
            continue;
        }
        if let Some(graduated) = max_bump_type(
            lane_consumed.iter().filter_map(|intent| ctx.intent_bump_for(intent, dir)),
        ) {
            bump_at_least(&mut state, dir, graduated, ReleaseCause::Intent);
        }
    }

    let cumulative_bump = |dir: &str, planned: ReleaseBumpType| -> ReleaseBumpType {
        lane_consumed_by_dir
            .get(dir)
            .into_iter()
            .flatten()
            .filter_map(|intent| ctx.intent_bump_for(intent, dir))
            .filter_map(IntentBumpType::release)
            .chain([planned])
            .max()
            .unwrap_or(planned)
    };

    let mut new_versions: BTreeMap<String, String> = BTreeMap::new();
    let compute_versions =
        |state: &BTreeMap<String, BumpState>, new_versions: &mut BTreeMap<String, String>| {
            new_versions.clear();
            for (dir, pkg_state) in state {
                let participant = &participants[dir.as_str()];
                new_versions.insert(
                    dir.clone(),
                    compute_new_version(
                        participant.current_version,
                        pkg_state.bump_type,
                        ctx.lanes_by_dir.get(dir).map(String::as_str),
                        cumulative_bump(dir, pkg_state.bump_type),
                        ctx.opts.unpublished_dirs.contains(dir),
                    ),
                );
            }
            apply_fixed_group_versions(
                participants,
                state,
                new_versions,
                &cumulative_bump,
                ctx.fixed_groups,
                ctx.lanes_by_dir,
            );
            apply_epic_band_versions(
                participants,
                state,
                new_versions,
                ctx.epics,
                ctx.lanes_by_dir,
            );
        };

    let mut changed = true;
    while changed {
        changed = false;
        compute_versions(&state, &mut new_versions);

        let mut forced: Vec<(&str, String, String)> = Vec::new();
        for dependent in participants.values() {
            for dep in &dependent.internal_deps {
                let Some(target) = participants.get(dep.target_dir.as_str()) else {
                    continue;
                };
                let Some(target_new_version) = new_versions.get(&dep.target_dir) else {
                    continue;
                };
                let Some(materialized) =
                    materialize_workspace_range(dep.spec, target.current_version)
                else {
                    continue;
                };
                if range_accepts(&materialized, target_new_version) {
                    continue;
                }
                forced.push((
                    dependent.dir.as_str(),
                    dep.target_name.clone(),
                    target_new_version.clone(),
                ));
            }
        }
        for (dependent_dir, target_name, target_new_version) in forced {
            if bump_at_least(
                &mut state,
                dependent_dir,
                ReleaseBumpType::Patch,
                ReleaseCause::Dependencies,
            ) {
                changed = true;
            }
            state
                .get_mut(dependent_dir)
                .expect("bump_at_least inserted the state")
                .dependency_updates
                .insert(target_name, target_new_version);
        }

        for group in ctx.fixed_groups {
            let Some(group_bump) = max_bump_type_of(
                group.iter().filter_map(|dir| state.get(dir).map(|entry| entry.bump_type)),
            ) else {
                continue;
            };
            for dir in group {
                if bump_at_least(&mut state, dir, group_bump, ReleaseCause::Fixed) {
                    changed = true;
                }
            }
        }

        // When the lead crosses to a new stable major, every member re-bases
        // to the band floor. Seed a release for each so the override in
        // apply_epic_band_versions has a version to replace and dependents
        // propagate.
        for epic in ctx.epics {
            if epic_rebase_floor(epic, participants, &new_versions).is_none() {
                continue;
            }
            for member_dir in &epic.member_dirs {
                if bump_at_least(&mut state, member_dir, ReleaseBumpType::Major, ReleaseCause::Epic)
                {
                    changed = true;
                }
            }
        }
    }
    compute_versions(&state, &mut new_versions);

    let mut releases: Vec<PlannedRelease> = state
        .iter()
        .map(|(dir, pkg_state)| {
            let participant = &participants[dir.as_str()];
            let mut consumed_for_changelog: Vec<ChangeIntent> = pending_by_dir
                .get(dir)
                .map(|intents| intents.iter().map(|&intent| intent.clone()).collect())
                .unwrap_or_default();
            if !ctx.lanes_by_dir.contains_key(dir)
                && let Some(lane_consumed) = lane_consumed_by_dir.get(dir)
            {
                consumed_for_changelog.extend(lane_consumed.iter().map(|&intent| intent.clone()));
            }
            PlannedRelease {
                name: participant.name.to_string(),
                dir: dir.clone(),
                root_dir: participant.root_dir.to_path_buf(),
                current_version: participant.current_version.to_string(),
                new_version: match &ctx.opts.snapshot_suffix {
                    Some(suffix) => format!("0.0.0-{suffix}"),
                    None => new_versions[dir].clone(),
                },
                bump_type: pkg_state.bump_type,
                intents: consumed_for_changelog,
                dependency_updates: pkg_state
                    .dependency_updates
                    .iter()
                    .map(|(dep_name, new_version)| DependencyUpdate {
                        name: dep_name.clone(),
                        new_version: new_version.clone(),
                    })
                    .collect(),
                causes: pkg_state.causes.iter().copied().collect(),
            }
        })
        .collect();
    releases
        .sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.dir.cmp(&right.dir)));

    assert_no_duplicate_release_identity(&releases)?;
    if ctx.opts.snapshot_suffix.is_none() {
        enforce_epic_bands(ctx.epics, participants, &new_versions)?;
        enforce_max_bump(&releases, ctx.versioning)?;
    }

    Ok(ReleasePlan { releases })
}

/// A published `package@version` identifies exactly one artifact, so two
/// projects that share a name cannot both release the same version — the
/// registry would reject the second publish, and the name-keyed ledger entry
/// would collide. Caught here, before any manifest is written, naming both
/// directories.
fn assert_no_duplicate_release_identity(
    releases: &[PlannedRelease],
) -> Result<(), VersioningError> {
    let mut by_identity: HashMap<String, String> = HashMap::new();
    for release in releases {
        let identity = format!("{}@{}", release.name, release.new_version);
        if let Some(other) = by_identity.insert(identity.clone(), release.dir.clone()) {
            return Err(VersioningError::DuplicateRelease {
                identity,
                first_dir: other,
                second_dir: release.dir.clone(),
            });
        }
    }
    Ok(())
}

fn bump_at_least(
    state: &mut BTreeMap<String, BumpState>,
    dir: &str,
    bump_type: ReleaseBumpType,
    cause: ReleaseCause,
) -> bool {
    let Some(existing) = state.get_mut(dir) else {
        state.insert(
            dir.to_string(),
            BumpState {
                bump_type,
                causes: BTreeSet::from([cause]),
                dependency_updates: BTreeMap::new(),
            },
        );
        return true;
    };
    existing.causes.insert(cause);
    if bump_type > existing.bump_type {
        existing.bump_type = bump_type;
        return true;
    }
    false
}

fn collect_participants<'a>(
    projects: &'a [WorkspaceProject],
    workspace_dir: &Path,
    refs: &ProjectRefIndex,
    versioning: Option<&VersioningSettings>,
) -> Result<BTreeMap<String, Participant<'a>>, VersioningError> {
    let mut ignored_dirs: HashSet<String> = HashSet::new();
    for reference in versioning.map(|settings| settings.ignore.as_slice()).unwrap_or_default() {
        ignored_dirs.extend(resolve_config_ref(refs, reference, "versioning.ignore")?);
    }

    let mut participants = BTreeMap::new();
    for project in projects {
        let (Some(name), Some(version)) = (project.name.as_deref(), project.version.as_deref())
        else {
            continue;
        };
        let dir = to_project_dir(workspace_dir, &project.root_dir);
        // What cannot release is excluded automatically: unnamed and
        // versionless (private) packages, packages with non-semver
        // placeholder versions, and the explicitly frozen ones.
        if Version::parse(version).is_err() || ignored_dirs.contains(&dir) {
            continue;
        }
        participants.insert(
            dir.clone(),
            Participant {
                name,
                dir,
                root_dir: &project.root_dir,
                current_version: version,
                internal_deps: Vec::new(),
            },
        );
    }

    let participant_dirs: HashSet<String> = participants.keys().cloned().collect();
    for (project, dir) in
        projects.iter().map(|project| (project, to_project_dir(workspace_dir, &project.root_dir)))
    {
        if !participant_dirs.contains(&dir) {
            continue;
        }
        let mut internal_deps = Vec::new();
        for dep in &project.prod_dependencies {
            let Some(target_name) = internal_dep_target_name(&dep.alias, &dep.spec, refs) else {
                continue;
            };
            let target_dirs: Vec<String> = refs
                .name_to_dirs(&target_name)
                .into_iter()
                .filter(|target_dir| participant_dirs.contains(target_dir))
                .collect();
            if target_dirs.is_empty() {
                continue;
            }
            // A workspace: range naming an ambiguous package cannot be
            // linked at install time, so the release engine never
            // legitimately sees one.
            if target_dirs.len() > 1 {
                let participant = &participants[dir.as_str()];
                return Err(VersioningError::AmbiguousPackage {
                    context: format!("Package {} (./{})", participant.name, participant.dir),
                    reference: target_name,
                    dirs: target_dirs,
                });
            }
            internal_deps.push(InternalDep {
                target_dir: target_dirs.into_iter().next().expect("one element"),
                target_name,
                field: dep.field,
                alias: &dep.alias,
                spec: &dep.spec,
            });
        }
        participants.get_mut(dir.as_str()).expect("participant exists").internal_deps =
            internal_deps;
    }
    Ok(participants)
}

/// Decides whether a dependency entry points at a workspace package. Aliased
/// specs targeting somewhere else (`npm:`, `file:`, git URLs, ...) are
/// external even when the alias collides with a workspace package name; a
/// plain semver range or `catalog:` entry on a workspace name is internal —
/// it is exactly the declaration the workspace-protocol check must reject.
fn internal_dep_target_name(alias: &str, spec: &str, refs: &ProjectRefIndex) -> Option<String> {
    if let Some(rest) = spec.strip_prefix("workspace:") {
        let target_name = parse_workspace_spec_alias(rest).unwrap_or(alias);
        return (!refs.name_to_dirs(target_name).is_empty()).then(|| target_name.to_string());
    }
    if refs.name_to_dirs(alias).is_empty() {
        return None;
    }
    (spec.starts_with("catalog:") || Range::parse(spec).is_ok()).then(|| alias.to_string())
}

/// The alias of an aliased `workspace:<alias>@<range>` spec body, mirroring
/// `WorkspaceSpec.parse` from `@pnpm/workspace.spec-parser`: the alias must
/// not start with `.`, `_`, or `/` and ends at the last `@`.
fn parse_workspace_spec_alias(rest: &str) -> Option<&str> {
    let at_index = rest.rfind('@').filter(|&index| index > 0)?;
    let alias = &rest[..at_index];
    if alias.starts_with(['.', '_', '/']) || alias.chars().skip(1).any(|character| character == '@')
    {
        return None;
    }
    Some(alias)
}

/// Resolves a package reference from `versioning` configuration. An unknown
/// reference is skipped — configuration may outlive a removed project — but
/// an ambiguous name is an error: it cannot be attributed, and silence here
/// is exactly the name-keying flaw this engine exists to fix.
fn resolve_config_ref(
    refs: &ProjectRefIndex,
    reference: &str,
    setting_name: &str,
) -> Result<Vec<String>, VersioningError> {
    let dirs = refs.ref_to_dirs(reference);
    if dirs.len() > 1 {
        return Err(VersioningError::AmbiguousPackage {
            context: setting_name.to_string(),
            reference: reference.to_string(),
            dirs,
        });
    }
    Ok(dirs)
}

fn resolve_lanes(
    refs: &ProjectRefIndex,
    versioning: Option<&VersioningSettings>,
) -> Result<BTreeMap<String, String>, VersioningError> {
    let mut lanes_by_dir = BTreeMap::new();
    let Some(settings) = versioning else {
        return Ok(lanes_by_dir);
    };
    for (reference, lane) in &settings.lanes {
        if lane.eq_ignore_ascii_case("main") {
            return Err(VersioningError::InvalidLaneName {
                pkg_name: reference.clone(),
                lane: lane.clone(),
            });
        }
        for dir in resolve_config_ref(refs, reference, "versioning.lanes")? {
            lanes_by_dir.insert(dir, lane.clone());
        }
    }
    Ok(lanes_by_dir)
}

fn resolve_fixed_groups(
    refs: &ProjectRefIndex,
    participants: &BTreeMap<String, Participant<'_>>,
    versioning: Option<&VersioningSettings>,
) -> Result<Vec<Vec<String>>, VersioningError> {
    let mut groups = Vec::new();
    for group in versioning.map(|settings| settings.fixed.as_slice()).unwrap_or_default() {
        let mut dirs = Vec::new();
        for reference in group {
            for dir in resolve_config_ref(refs, reference, "versioning.fixed")? {
                if participants.contains_key(&dir) {
                    dirs.push(dir);
                }
            }
        }
        groups.push(dirs);
    }
    Ok(groups)
}

fn validate_fixed_group_lanes(
    fixed_groups: &[Vec<String>],
    lanes_by_dir: &BTreeMap<String, String>,
    versioning: Option<&VersioningSettings>,
) -> Result<(), VersioningError> {
    for (index, group) in fixed_groups.iter().enumerate() {
        let tags: HashSet<Option<&String>> =
            group.iter().map(|dir| lanes_by_dir.get(dir)).collect();
        if tags.len() > 1 {
            let declared =
                versioning.map(|settings| settings.fixed[index].clone()).unwrap_or_default();
            return Err(VersioningError::ConflictingConfig { group: declared });
        }
    }
    Ok(())
}

/// Resolves each configured epic to its lead directory and the set of member
/// directories its selectors match. The lead — a single named package with a
/// semver version — is excluded from its own membership. Membership selectors
/// match name globs, `./`-prefixed directory globs, and `!`-prefixed negations.
fn resolve_epics(
    refs: &ProjectRefIndex,
    participants: &BTreeMap<String, Participant<'_>>,
    versioning: Option<&VersioningSettings>,
) -> Result<Vec<ResolvedEpic>, VersioningError> {
    let mut epics = Vec::new();
    for epic in versioning.map(|settings| settings.epics.as_slice()).unwrap_or_default() {
        let lead_dir = resolve_config_ref(refs, &epic.lead, "versioning.epics lead")?
            .into_iter()
            .next()
            .filter(|dir| participants.contains_key(dir))
            .ok_or_else(|| VersioningError::EpicUnknownLead { lead: epic.lead.clone() })?;
        let selectors: Vec<EpicSelector> =
            epic.packages.iter().map(|selector| compile_epic_selector(selector)).collect();
        let mut member_dirs = HashSet::new();
        for participant in participants.values() {
            if participant.dir == lead_dir {
                continue;
            }
            if matches_epic_selectors(&selectors, &participant.dir, participant.name) {
                member_dirs.insert(participant.dir.clone());
            }
        }
        epics.push(ResolvedEpic { lead_ref: epic.lead.clone(), lead_dir, member_dirs });
    }
    Ok(epics)
}

struct EpicSelector {
    negated: bool,
    /// Whether the pattern matches a project's directory rather than its name.
    on_dir: bool,
    pattern: String,
}

fn compile_epic_selector(selector: &str) -> EpicSelector {
    let (negated, body) = match selector.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, selector),
    };
    let on_dir = is_dir_ref(body);
    let pattern = if on_dir { normalize_project_dir(body) } else { body.to_string() };
    EpicSelector { negated, on_dir, pattern }
}

/// Whether a project is an epic member under pnpm's order-dependent selector
/// rule: each matching selector overrides the previous verdict, so the last
/// one to match decides — a positive include or a `!` negation — mirroring
/// `@pnpm/config.matcher`, where a later include can re-include a package an
/// earlier negation excluded.
fn matches_epic_selectors(selectors: &[EpicSelector], dir: &str, name: &str) -> bool {
    let mut included = false;
    for selector in selectors {
        let input = if selector.on_dir { dir } else { name };
        if wildcard_match(&selector.pattern, input) {
            included = !selector.negated;
        }
    }
    included
}

/// Matches `input` against a pattern where `*` matches any run of characters
/// and every other character is literal, mirroring `@pnpm/config.matcher`'s
/// wildcard semantics so epic membership globs behave like the TypeScript
/// engine's.
fn wildcard_match(pattern: &str, input: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let input: Vec<char> = input.chars().collect();
    let (mut p, mut s) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while s < input.len() {
        if pattern.get(p) == Some(&'*') {
            star = Some(p);
            mark = s;
            p += 1;
        } else if pattern.get(p) == Some(&input[s]) {
            p += 1;
            s += 1;
        } else if let Some(star_p) = star {
            p = star_p + 1;
            mark += 1;
            s = mark;
        } else {
            return false;
        }
    }
    while pattern.get(p) == Some(&'*') {
        p += 1;
    }
    p == pattern.len()
}

/// Rejects epic configurations that cannot be attributed unambiguously: a
/// package matched by two epics, and a fixed group that straddles an epic
/// boundary (a group must sit entirely inside or entirely outside an epic, so
/// its members never disagree on whether they are band-constrained).
fn validate_epics(
    epics: &[ResolvedEpic],
    fixed_groups: &[Vec<String>],
) -> Result<(), VersioningError> {
    let mut epic_of_member: HashMap<&str, &str> = HashMap::new();
    for epic in epics {
        for member_dir in &epic.member_dirs {
            if let Some(other) = epic_of_member.get(member_dir.as_str())
                && *other != epic.lead_ref
            {
                return Err(VersioningError::EpicOverlap {
                    member_dir: member_dir.clone(),
                    first_lead: (*other).to_string(),
                    second_lead: epic.lead_ref.clone(),
                });
            }
            epic_of_member.insert(member_dir.as_str(), &epic.lead_ref);
        }
    }

    for epic in epics {
        for group in fixed_groups {
            if !group.iter().any(|dir| epic.member_dirs.contains(dir)) {
                continue;
            }
            let outsiders: Vec<String> = group
                .iter()
                .filter(|dir| !epic.member_dirs.contains(*dir))
                .map(|dir| format!("./{dir}"))
                .collect();
            if !outsiders.is_empty() {
                return Err(VersioningError::EpicFixedGroupConflict {
                    lead: epic.lead_ref.clone(),
                    outsiders: outsiders.join(", "),
                });
            }
        }
    }
    Ok(())
}

/// Resolves every intent's package references to participant directories,
/// validating along the way: unknown references and names matching several
/// projects are hard errors, and a release can only be demanded from a
/// participant — otherwise the intent could never be consumed and the file
/// would linger forever. A `none` decline is fine for any workspace package.
fn resolve_intents(
    intents: &[ChangeIntent],
    refs: &ProjectRefIndex,
    participants: &BTreeMap<String, Participant<'_>>,
) -> Result<HashMap<String, BTreeMap<String, IntentBumpType>>, VersioningError> {
    let mut intent_bumps = HashMap::new();
    for intent in intents {
        let mut by_dir: BTreeMap<String, IntentBumpType> = BTreeMap::new();
        for (reference, bump_type) in &intent.releases {
            let dirs = refs.ref_to_dirs(reference);
            if dirs.is_empty() {
                return Err(VersioningError::UnknownPackage {
                    file_path: intent.file_path.clone(),
                    pkg_name: reference.clone(),
                });
            }
            if dirs.len() > 1 {
                return Err(VersioningError::AmbiguousPackage {
                    context: format!("Change intent file {}", intent.file_path.display()),
                    reference: reference.clone(),
                    dirs,
                });
            }
            let dir = dirs.into_iter().next().expect("one element");
            if *bump_type != IntentBumpType::None && !participants.contains_key(&dir) {
                return Err(VersioningError::UnreleasablePackage {
                    file_path: intent.file_path.clone(),
                    pkg_name: reference.clone(),
                    bump_type: bump_type.to_string(),
                });
            }
            let entry = by_dir.entry(dir).or_insert(*bump_type);
            if bump_release_order(*bump_type) > bump_release_order(*entry) {
                *entry = *bump_type;
            }
        }
        intent_bumps.insert(intent.id.clone(), by_dir);
    }
    Ok(intent_bumps)
}

fn bump_release_order(bump_type: IntentBumpType) -> u8 {
    match bump_type.release() {
        None => 0,
        Some(ReleaseBumpType::Patch) => 1,
        Some(ReleaseBumpType::Minor) => 2,
        Some(ReleaseBumpType::Major) => 3,
    }
}

fn assert_internal_deps_use_workspace_protocol(
    participants: &BTreeMap<String, Participant<'_>>,
) -> Result<(), VersioningError> {
    for participant in participants.values() {
        for dep in &participant.internal_deps {
            if !dep.spec.starts_with("workspace:") {
                return Err(VersioningError::InternalRange {
                    pkg_name: participant.name.to_string(),
                    alias: dep.alias.to_string(),
                    field: dep.field.to_string(),
                    spec: dep.spec.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn collect_pending_intents<'i>(
    ctx: &AssembleContext<'i>,
) -> BTreeMap<String, Vec<&'i ChangeIntent>> {
    let mut pending = BTreeMap::new();
    let empty = PackageConsumption::default();
    for dir in ctx.participants.keys() {
        let consumed = ctx.consumption.get(dir).unwrap_or(&empty);
        let pkg_intents: Vec<&ChangeIntent> = ctx
            .intents
            .iter()
            .filter(|intent| {
                ctx.intent_bump_for(intent, dir).is_some_and(|bump| bump != IntentBumpType::None)
                    && !consumed.all_ids.contains(&intent.id)
            })
            .collect();
        if !pkg_intents.is_empty() {
            pending.insert(dir.clone(), pkg_intents);
        }
    }
    pending
}

/// Intents already consumed by prereleases of a package that has not
/// graduated to a stable version yet. They participate in the cumulative
/// bump computation of the package's lane and compose the stable changelog
/// section at graduation.
fn collect_lane_consumed_intents<'i>(
    ctx: &AssembleContext<'i>,
) -> BTreeMap<String, Vec<&'i ChangeIntent>> {
    let mut lane_consumed = BTreeMap::new();
    for dir in ctx.participants.keys() {
        let Some(consumed) = ctx.consumption.get(dir) else {
            continue;
        };
        if consumed.prerelease_only_ids.is_empty() {
            continue;
        }
        let pkg_intents: Vec<&ChangeIntent> = ctx
            .intents
            .iter()
            .filter(|intent| {
                ctx.intent_bump_for(intent, dir).is_some_and(|bump| bump != IntentBumpType::None)
                    && consumed.prerelease_only_ids.contains(&intent.id)
            })
            .collect();
        if !pkg_intents.is_empty() {
            lane_consumed.insert(dir.clone(), pkg_intents);
        }
    }
    lane_consumed
}

fn max_bump_type(types: impl Iterator<Item = IntentBumpType>) -> Option<ReleaseBumpType> {
    max_bump_type_of(types.filter_map(IntentBumpType::release))
}

fn max_bump_type_of(types: impl Iterator<Item = ReleaseBumpType>) -> Option<ReleaseBumpType> {
    types.max()
}

fn compute_new_version(
    current: &str,
    bump_type: ReleaseBumpType,
    lane_tag: Option<&str>,
    cumulative_bump: ReleaseBumpType,
    first_release: bool,
) -> String {
    let current_version = Version::parse(current).expect("participants have valid versions");
    let Some(lane_tag) = lane_tag else {
        if first_release {
            return current.to_string();
        }
        if current_version.pre_release.is_empty() {
            return inc_stable(&current_version, bump_type);
        }
        // Graduation: the accumulated stable version the lane was building
        // toward.
        return escalate_stable_target(&stable_part(&current_version), cumulative_bump);
    };
    let target = if first_release {
        stable_part(&current_version)
    } else if current_version.pre_release.is_empty() {
        inc_stable(&current_version, cumulative_bump)
    } else {
        escalate_stable_target(&stable_part(&current_version), cumulative_bump)
    };
    let next_n = next_prerelease_number(&current_version, &target, lane_tag);
    format!("{target}-{lane_tag}.{next_n}")
}

fn inc_stable(version: &Version, bump_type: ReleaseBumpType) -> String {
    match bump_type {
        ReleaseBumpType::Major => format!("{}.0.0", version.major + 1),
        ReleaseBumpType::Minor => format!("{}.{}.0", version.major, version.minor + 1),
        ReleaseBumpType::Patch => {
            format!("{}.{}.{}", version.major, version.minor, version.patch + 1)
        }
    }
}

/// Re-derives the stable version a lane is building toward when the
/// cumulative bump escalates. The invariant: the stable part of the current
/// prerelease already reflects the previous cumulative bump applied to the
/// version the lane started from, so only an escalation changes it.
fn escalate_stable_target(target: &str, cumulative_bump: ReleaseBumpType) -> String {
    let target_version = Version::parse(target).expect("stable parts are valid versions");
    match cumulative_bump {
        ReleaseBumpType::Major => {
            if target_version.minor == 0 && target_version.patch == 0 {
                target.to_string()
            } else {
                format!("{}.0.0", target_version.major + 1)
            }
        }
        ReleaseBumpType::Minor => {
            if target_version.patch == 0 {
                target.to_string()
            } else {
                format!("{}.{}.0", target_version.major, target_version.minor + 1)
            }
        }
        ReleaseBumpType::Patch => target.to_string(),
    }
}

fn stable_part(version: &Version) -> String {
    format!("{}.{}.{}", version.major, version.minor, version.patch)
}

fn next_prerelease_number(current: &Version, target: &str, lane_tag: &str) -> u64 {
    if current.pre_release.is_empty() || stable_part(current) != target {
        return 0;
    }
    // semver parses an all-digit prerelease identifier as a number, so the
    // tag comparison must not be strict about the identifier kind.
    let current_tag = match current.pre_release.first() {
        Some(Identifier::AlphaNumeric(tag)) => tag.clone(),
        Some(Identifier::Numeric(tag)) => tag.to_string(),
        None => return 0,
    };
    if current_tag != lane_tag {
        return 0;
    }
    match current.pre_release.get(1) {
        Some(Identifier::Numeric(current_n)) => current_n + 1,
        _ => 0,
    }
}

fn apply_fixed_group_versions(
    participants: &BTreeMap<String, Participant<'_>>,
    state: &BTreeMap<String, BumpState>,
    new_versions: &mut BTreeMap<String, String>,
    cumulative_bump: &dyn Fn(&str, ReleaseBumpType) -> ReleaseBumpType,
    fixed_groups: &[Vec<String>],
    lanes_by_dir: &BTreeMap<String, String>,
) {
    for group in fixed_groups {
        let Some(group_bump) =
            max_bump_type_of(group.iter().filter_map(|dir| {
                state.get(dir).map(|entry| cumulative_bump(dir, entry.bump_type))
            }))
        else {
            continue;
        };

        let highest_current = group
            .iter()
            .map(|dir| {
                Version::parse(participants[dir.as_str()].current_version)
                    .expect("participants have valid versions")
            })
            .max();
        let Some(highest_current) = highest_current else {
            continue;
        };
        let target = if highest_current.pre_release.is_empty() {
            inc_stable(&highest_current, group_bump)
        } else {
            escalate_stable_target(&stable_part(&highest_current), group_bump)
        };

        let lane_tag = group.first().and_then(|dir| lanes_by_dir.get(dir));
        let shared_version = match lane_tag {
            Some(lane_tag) => {
                let next_n = group
                    .iter()
                    .map(|dir| {
                        let current = Version::parse(participants[dir.as_str()].current_version)
                            .expect("participants have valid versions");
                        next_prerelease_number(&current, &target, lane_tag)
                    })
                    .max()
                    .unwrap_or(0);
                format!("{target}-{lane_tag}.{next_n}")
            }
            None => target,
        };
        for dir in group {
            if state.contains_key(dir) {
                new_versions.insert(dir.clone(), shared_version.clone());
            }
        }
    }
}

/// The band floor (`new_major × 100`) an epic re-bases its members to, or
/// `None` when no re-base is due. A re-base fires only when the lead releases
/// to a new, higher *stable* major in this plan; a prerelease lead version
/// (the lead on a lane) defers the re-base until its stable release.
fn epic_rebase_floor(
    epic: &ResolvedEpic,
    participants: &BTreeMap<String, Participant<'_>>,
    new_versions: &BTreeMap<String, String>,
) -> Option<u64> {
    let lead = participants.get(epic.lead_dir.as_str())?;
    let new_lead = Version::parse(new_versions.get(&epic.lead_dir)?).ok()?;
    if !new_lead.pre_release.is_empty() {
        return None;
    }
    let current_major = Version::parse(lead.current_version).ok()?.major;
    (new_lead.major > current_major).then_some(new_lead.major * 100)
}

/// Overrides the computed version of every bumped epic member with the band
/// floor when its lead crosses to a new stable major. A member on a lane
/// re-bases to a prerelease of the floor; every other member to `floor.0.0`.
fn apply_epic_band_versions(
    participants: &BTreeMap<String, Participant<'_>>,
    state: &BTreeMap<String, BumpState>,
    new_versions: &mut BTreeMap<String, String>,
    epics: &[ResolvedEpic],
    lanes_by_dir: &BTreeMap<String, String>,
) {
    for epic in epics {
        let Some(floor) = epic_rebase_floor(epic, participants, new_versions) else {
            continue;
        };
        let target = format!("{floor}.0.0");
        for member_dir in &epic.member_dirs {
            if !state.contains_key(member_dir) {
                continue;
            }
            let version = match lanes_by_dir.get(member_dir) {
                None => target.clone(),
                Some(lane_tag) => {
                    let current = Version::parse(participants[member_dir.as_str()].current_version)
                        .expect("participants have valid versions");
                    let next_n = next_prerelease_number(&current, &target, lane_tag);
                    format!("{target}-{lane_tag}.{next_n}")
                }
            };
            new_versions.insert(member_dir.clone(), version);
        }
    }
}

/// The band of member majors an epic permits, `[lead_major*100,
/// lead_major*100+99]`, where `lead_major` is the major the plan establishes
/// for the lead — its re-based major when the lead crosses to a new stable
/// major, otherwise the lead's current major (a prerelease lead does not open
/// the next band).
fn epic_band_major(
    epic: &ResolvedEpic,
    participants: &BTreeMap<String, Participant<'_>>,
    new_versions: &BTreeMap<String, String>,
) -> u64 {
    match epic_rebase_floor(epic, participants, new_versions) {
        Some(floor) => floor / 100,
        None => {
            Version::parse(participants[epic.lead_dir.as_str()].current_version)
                .expect("participants have valid versions")
                .major
        }
    }
}

/// Enforces that every released member's new major stays inside its epic's
/// band. The re-base already keeps members in band when the lead moves; this
/// guards the other direction — an ordinary `major` intent that would carry a
/// member over the band ceiling (`1199.x` -> `1200.0.0` while the lead is
/// still on 11) is rejected rather than silently landing in the next band.
fn enforce_epic_bands(
    epics: &[ResolvedEpic],
    participants: &BTreeMap<String, Participant<'_>>,
    new_versions: &BTreeMap<String, String>,
) -> Result<(), VersioningError> {
    for epic in epics {
        let band_major = epic_band_major(epic, participants, new_versions);
        let low = band_major * 100;
        let high = low + 99;
        for member_dir in &epic.member_dirs {
            let Some(member_version) = new_versions.get(member_dir) else {
                continue;
            };
            let member_major =
                Version::parse(member_version).expect("participants have valid versions").major;
            if member_major < low || member_major > high {
                return Err(VersioningError::EpicOutOfBand {
                    pkg_name: participants[member_dir.as_str()].name.to_string(),
                    new_version: member_version.clone(),
                    member_major,
                    lead: epic.lead_ref.clone(),
                    band_major,
                });
            }
        }
    }
    Ok(())
}

/// The range that pnpm materializes for a workspace: spec at pack time,
/// given the dependency's version at the dependent's previous release.
/// Dependent propagation republishes the dependent whenever the dependency's
/// new version falls outside this range.
#[must_use]
pub fn materialize_workspace_range(spec: &str, dep_current_version: &str) -> Option<String> {
    let rest = spec.strip_prefix("workspace:")?;
    let range = match parse_workspace_spec_alias(rest) {
        Some(alias) => &rest[alias.len() + 1..],
        None => rest,
    };
    Some(match range {
        "^" => format!("^{dep_current_version}"),
        "~" => format!("~{dep_current_version}"),
        "*" | "" => dep_current_version.to_string(),
        explicit => explicit.to_string(),
    })
}

fn range_accepts(range: &str, version: &str) -> bool {
    let (Ok(range), Ok(version)) = (Range::parse(range), Version::parse(version)) else {
        return false;
    };
    version.satisfies(&range)
}

fn enforce_max_bump(
    releases: &[PlannedRelease],
    versioning: Option<&VersioningSettings>,
) -> Result<(), VersioningError> {
    let Some(max_bump) = versioning.and_then(|settings| settings.max_bump) else {
        return Ok(());
    };
    for release in releases {
        let effective_bump = effective_bump_class(release);
        if effective_bump <= max_bump {
            continue;
        }
        let intent_files: Vec<String> = release
            .intents
            .iter()
            .filter(|intent| {
                intent.releases.values().any(|bump| bump.release() == Some(effective_bump))
            })
            .map(|intent| intent.file_path.display().to_string())
            .collect();
        let raised_by = if intent_files.is_empty() {
            format!(
                "constraint chain: {}",
                release.causes.iter().map(ToString::to_string).collect::<Vec<String>>().join(", "),
            )
        } else {
            format!("intent file(s) {}", intent_files.join(", "))
        };
        return Err(VersioningError::MaxBumpExceeded {
            pkg_name: release.name.clone(),
            bump_type: effective_bump.to_string(),
            max_bump: max_bump.to_string(),
            raised_by,
        });
    }
    Ok(())
}

/// The bump class a release actually applies. Fixed-group version sharing
/// and lane escalation can move a version further than the package's own
/// declared or propagated bump, so the cap compares against the real
/// distance between the current and the new version as well.
fn effective_bump_class(release: &PlannedRelease) -> ReleaseBumpType {
    let (Ok(current), Ok(new_version)) =
        (Version::parse(&release.current_version), Version::parse(&release.new_version))
    else {
        return release.bump_type;
    };
    let diff_class = if new_version.major != current.major {
        Some(ReleaseBumpType::Major)
    } else if new_version.minor != current.minor {
        Some(ReleaseBumpType::Minor)
    } else if new_version.patch != current.patch {
        Some(ReleaseBumpType::Patch)
    } else {
        None
    };
    diff_class.into_iter().chain([release.bump_type]).max().unwrap_or(release.bump_type)
}

#[cfg(test)]
mod tests;
