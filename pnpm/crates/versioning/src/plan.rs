use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::PathBuf,
};

use derive_more::Display;
use node_semver::{Identifier, Range, Version};

use crate::{
    error::VersioningError,
    intents::{ChangeIntent, IntentBumpType},
    ledger::{Ledger, PackageConsumption, build_consumption_index},
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
/// `dependencies < fixed < intent`.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReleaseCause {
    #[display("dependencies")]
    Dependencies,
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
    pub root_dir: PathBuf,
    pub current_version: String,
    pub new_version: String,
    pub bump_type: ReleaseBumpType,
    /// The intent files this release consumes for this package: the pending
    /// ones, plus — when the release graduates the package off a prerelease
    /// line — the ones the ledger recorded against the line's prerelease
    /// versions.
    pub intents: Vec<ChangeIntent>,
    pub dependency_updates: Vec<DependencyUpdate>,
    pub causes: Vec<ReleaseCause>,
}

#[derive(Debug, Default, Clone)]
pub struct ReleasePlan {
    pub releases: Vec<PlannedRelease>,
}

#[derive(Debug, Clone, Default)]
pub struct AssembleReleasePlanOptions {
    /// Package names selected with --filter. The plan is narrowed to the
    /// selected packages' portion of the pending work, expanded with their
    /// fixed-group companions and range-invalidated dependents.
    pub filter: Option<HashSet<String>>,
    /// When set, every planned release gets the version `0.0.0-<suffix>`
    /// instead of the computed one, matching snapshot releases.
    pub snapshot_suffix: Option<String>,
}

pub fn assemble_release_plan(
    projects: &[WorkspaceProject],
    intents: &[ChangeIntent],
    ledger: &Ledger,
    versioning: Option<&VersioningSettings>,
    opts: &AssembleReleasePlanOptions,
) -> Result<ReleasePlan, VersioningError> {
    let participants = collect_participants(projects, versioning);
    validate_versioning_config(&participants, versioning)?;
    validate_intents(intents, projects, &participants)?;
    assert_internal_deps_use_workspace_protocol(&participants)?;
    let consumption = build_consumption_index(ledger);

    let mut selection = opts.filter.clone();
    loop {
        let plan =
            assemble(&participants, &consumption, selection.as_ref(), intents, versioning, opts)?;
        let Some(selected) = &mut selection else {
            return Ok(plan);
        };
        let before = selected.len();
        selected.extend(plan.releases.iter().map(|release| release.name.clone()));
        if selected.len() == before {
            return Ok(plan);
        }
    }
}

struct Participant<'a> {
    name: &'a str,
    root_dir: &'a PathBuf,
    current_version: &'a str,
    internal_deps: Vec<InternalDep<'a>>,
}

struct InternalDep<'a> {
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

fn assemble(
    participants: &BTreeMap<&str, Participant<'_>>,
    consumption: &HashMap<String, PackageConsumption>,
    selection: Option<&HashSet<String>>,
    intents: &[ChangeIntent],
    versioning: Option<&VersioningSettings>,
    opts: &AssembleReleasePlanOptions,
) -> Result<ReleasePlan, VersioningError> {
    let pending_by_pkg = collect_pending_intents(participants, intents, consumption);
    let line_consumed_by_pkg = collect_line_consumed_intents(participants, intents, consumption);
    let empty_prereleases = indexmap::IndexMap::new();
    let prereleases = versioning.map_or(&empty_prereleases, |settings| &settings.prereleases);

    let mut state: BTreeMap<String, BumpState> = BTreeMap::new();

    for (&name, pending) in &pending_by_pkg {
        if selection.is_some_and(|selected| !selected.contains(name)) {
            continue;
        }
        if let Some(direct) = max_bump_type(pending.iter().map(|intent| intent.releases[name])) {
            bump_at_least(&mut state, name, direct, ReleaseCause::Intent);
        }
    }

    // A package that left its prerelease line releases the accumulated stable
    // version even when no new intents are pending.
    for (&name, line_consumed) in &line_consumed_by_pkg {
        if selection.is_some_and(|selected| !selected.contains(name)) {
            continue;
        }
        if prereleases.contains_key(name) {
            continue;
        }
        if let Some(graduated) =
            max_bump_type(line_consumed.iter().map(|intent| intent.releases[name]))
        {
            bump_at_least(&mut state, name, graduated, ReleaseCause::Intent);
        }
    }

    let mut new_versions: BTreeMap<String, String> = BTreeMap::new();
    let compute_versions =
        |state: &BTreeMap<String, BumpState>, new_versions: &mut BTreeMap<String, String>| {
            new_versions.clear();
            for (name, pkg_state) in state {
                let participant = &participants[name.as_str()];
                let cumulative =
                    cumulative_bump_type(name, pkg_state.bump_type, &line_consumed_by_pkg);
                new_versions.insert(
                    name.clone(),
                    compute_new_version(
                        participant.current_version,
                        pkg_state.bump_type,
                        prereleases.get(name).map(String::as_str),
                        cumulative,
                    ),
                );
            }
            apply_fixed_group_versions(
                participants,
                state,
                new_versions,
                &line_consumed_by_pkg,
                versioning,
            );
        };

    let mut changed = true;
    while changed {
        changed = false;
        compute_versions(&state, &mut new_versions);

        let mut forced: Vec<(&str, String, String)> = Vec::new();
        for dependent in participants.values() {
            for dep in &dependent.internal_deps {
                let Some(target) = participants.get(dep.target_name.as_str()) else {
                    continue;
                };
                let Some(target_new_version) = new_versions.get(&dep.target_name) else {
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
                forced.push((dependent.name, dep.target_name.clone(), target_new_version.clone()));
            }
        }
        for (dependent_name, target_name, target_new_version) in forced {
            if bump_at_least(
                &mut state,
                dependent_name,
                ReleaseBumpType::Patch,
                ReleaseCause::Dependencies,
            ) {
                changed = true;
            }
            state
                .get_mut(dependent_name)
                .expect("bump_at_least inserted the state")
                .dependency_updates
                .insert(target_name, target_new_version);
        }

        for group in versioning.map(|settings| settings.fixed.as_slice()).unwrap_or_default() {
            let members: Vec<&str> = group
                .iter()
                .map(String::as_str)
                .filter(|name| participants.contains_key(name))
                .collect();
            let Some(group_bump) = max_bump_type_of(
                members.iter().filter_map(|&name| state.get(name).map(|entry| entry.bump_type)),
            ) else {
                continue;
            };
            for name in members {
                if bump_at_least(&mut state, name, group_bump, ReleaseCause::Fixed) {
                    changed = true;
                }
            }
        }
    }
    compute_versions(&state, &mut new_versions);

    let mut releases: Vec<PlannedRelease> = state
        .iter()
        .map(|(name, pkg_state)| {
            let name = name.as_str();
            let participant = &participants[name];
            let mut consumed_for_changelog: Vec<ChangeIntent> = pending_by_pkg
                .get(name)
                .map(|intents| intents.iter().map(|&intent| intent.clone()).collect())
                .unwrap_or_default();
            if !prereleases.contains_key(name)
                && let Some(line_consumed) = line_consumed_by_pkg.get(name)
            {
                consumed_for_changelog.extend(line_consumed.iter().map(|&intent| intent.clone()));
            }
            PlannedRelease {
                name: name.to_string(),
                root_dir: participant.root_dir.clone(),
                current_version: participant.current_version.to_string(),
                new_version: match &opts.snapshot_suffix {
                    Some(suffix) => format!("0.0.0-{suffix}"),
                    None => new_versions[name].clone(),
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
    releases.sort_by(|left, right| left.name.cmp(&right.name));

    if opts.snapshot_suffix.is_none() {
        enforce_max_bump(&releases, versioning)?;
    }

    Ok(ReleasePlan { releases })
}

fn bump_at_least(
    state: &mut BTreeMap<String, BumpState>,
    name: &str,
    bump_type: ReleaseBumpType,
    cause: ReleaseCause,
) -> bool {
    let Some(existing) = state.get_mut(name) else {
        state.insert(
            name.to_string(),
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
    versioning: Option<&VersioningSettings>,
) -> BTreeMap<&'a str, Participant<'a>> {
    let ignored: HashSet<&str> = versioning
        .map(|settings| settings.ignore.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let names: HashSet<&str> =
        projects.iter().filter_map(|project| project.name.as_deref()).collect();

    let mut participants = BTreeMap::new();
    for project in projects {
        let (Some(name), Some(version)) = (project.name.as_deref(), project.version.as_deref())
        else {
            continue;
        };
        // What cannot release is excluded automatically: unnamed and
        // versionless (private) packages, packages with non-semver
        // placeholder versions, and the explicitly frozen ones.
        if Version::parse(version).is_err() || ignored.contains(name) {
            continue;
        }
        let internal_deps = project
            .prod_dependencies
            .iter()
            .filter_map(|dep| {
                let target_name = resolve_internal_dep_target(&dep.alias, &dep.spec, &names)?;
                if ignored.contains(target_name.as_str()) {
                    return None;
                }
                Some(InternalDep {
                    target_name,
                    field: dep.field,
                    alias: &dep.alias,
                    spec: &dep.spec,
                })
            })
            .collect();
        participants.insert(
            name,
            Participant {
                name,
                root_dir: &project.root_dir,
                current_version: version,
                internal_deps,
            },
        );
    }
    participants
}

/// Decides whether a dependency entry points at a workspace package. Aliased
/// specs targeting somewhere else (`npm:`, `file:`, git URLs, …) are external
/// even when the alias collides with a workspace package name; a plain semver
/// range or `catalog:` entry on a workspace name is internal — it is exactly
/// the declaration the workspace-protocol check must reject.
fn resolve_internal_dep_target(
    alias: &str,
    spec: &str,
    workspace_names: &HashSet<&str>,
) -> Option<String> {
    if let Some(rest) = spec.strip_prefix("workspace:") {
        let target_name = parse_workspace_spec_alias(rest).unwrap_or(alias);
        return workspace_names.contains(target_name).then(|| target_name.to_string());
    }
    if !workspace_names.contains(alias) {
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
    if alias.starts_with(['.', '_', '/']) || alias[1..].contains('@') {
        return None;
    }
    Some(alias)
}

fn validate_versioning_config(
    participants: &BTreeMap<&str, Participant<'_>>,
    versioning: Option<&VersioningSettings>,
) -> Result<(), VersioningError> {
    let Some(settings) = versioning else {
        return Ok(());
    };
    for group in &settings.fixed {
        let tags: HashSet<Option<&String>> = group
            .iter()
            .filter(|name| participants.contains_key(name.as_str()))
            .map(|name| settings.prereleases.get(name))
            .collect();
        if tags.len() > 1 {
            return Err(VersioningError::ConflictingConfig { group: group.clone() });
        }
    }
    Ok(())
}

fn validate_intents(
    intents: &[ChangeIntent],
    projects: &[WorkspaceProject],
    participants: &BTreeMap<&str, Participant<'_>>,
) -> Result<(), VersioningError> {
    let workspace_names: HashSet<&str> =
        projects.iter().filter_map(|project| project.name.as_deref()).collect();
    for intent in intents {
        for (pkg_name, bump_type) in &intent.releases {
            if !workspace_names.contains(pkg_name.as_str()) {
                return Err(VersioningError::UnknownPackage {
                    file_path: intent.file_path.clone(),
                    pkg_name: pkg_name.clone(),
                });
            }
            // A "none" decline is fine for any workspace package, but a
            // release can only be demanded from a participant — otherwise the
            // intent could never be consumed and the file would linger
            // forever.
            if *bump_type != IntentBumpType::None && !participants.contains_key(pkg_name.as_str()) {
                return Err(VersioningError::UnreleasablePackage {
                    file_path: intent.file_path.clone(),
                    pkg_name: pkg_name.clone(),
                    bump_type: bump_type.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn assert_internal_deps_use_workspace_protocol(
    participants: &BTreeMap<&str, Participant<'_>>,
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

fn collect_pending_intents<'a, 'i>(
    participants: &BTreeMap<&'a str, Participant<'a>>,
    intents: &'i [ChangeIntent],
    consumption: &HashMap<String, PackageConsumption>,
) -> BTreeMap<&'a str, Vec<&'i ChangeIntent>> {
    let mut pending = BTreeMap::new();
    for &name in participants.keys() {
        let consumed_ids = consumption.get(name).map(|entry| &entry.all_ids);
        let pkg_intents: Vec<&ChangeIntent> = intents
            .iter()
            .filter(|intent| {
                intent.releases.get(name).is_some_and(|bump| *bump != IntentBumpType::None)
                    && !consumed_ids.is_some_and(|ids| ids.contains(&intent.id))
            })
            .collect();
        if !pkg_intents.is_empty() {
            pending.insert(name, pkg_intents);
        }
    }
    pending
}

/// Intents already consumed by prereleases of a package that has not
/// graduated to a stable version yet. They participate in the cumulative bump
/// computation of the package's prerelease line and compose the stable
/// changelog section at graduation.
fn collect_line_consumed_intents<'a, 'i>(
    participants: &BTreeMap<&'a str, Participant<'a>>,
    intents: &'i [ChangeIntent],
    consumption: &HashMap<String, PackageConsumption>,
) -> BTreeMap<&'a str, Vec<&'i ChangeIntent>> {
    let mut line_consumed = BTreeMap::new();
    for &name in participants.keys() {
        let Some(prerelease_only) = consumption.get(name).map(|entry| &entry.prerelease_only_ids)
        else {
            continue;
        };
        if prerelease_only.is_empty() {
            continue;
        }
        let pkg_intents: Vec<&ChangeIntent> = intents
            .iter()
            .filter(|intent| {
                intent.releases.get(name).is_some_and(|bump| *bump != IntentBumpType::None)
                    && prerelease_only.contains(&intent.id)
            })
            .collect();
        if !pkg_intents.is_empty() {
            line_consumed.insert(name, pkg_intents);
        }
    }
    line_consumed
}

fn cumulative_bump_type(
    name: &str,
    planned: ReleaseBumpType,
    line_consumed_by_pkg: &BTreeMap<&str, Vec<&ChangeIntent>>,
) -> ReleaseBumpType {
    line_consumed_by_pkg
        .get(name)
        .into_iter()
        .flatten()
        .filter_map(|intent| intent.releases[name].release())
        .chain([planned])
        .max()
        .unwrap_or(planned)
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
    line_tag: Option<&str>,
    cumulative_bump: ReleaseBumpType,
) -> String {
    let current_version = Version::parse(current).expect("participants have valid versions");
    let Some(line_tag) = line_tag else {
        if current_version.pre_release.is_empty() {
            return inc_stable(&current_version, bump_type);
        }
        // Graduation: the accumulated stable version the prerelease line was
        // building toward.
        return escalate_stable_target(&stable_part(&current_version), cumulative_bump);
    };
    let target = if current_version.pre_release.is_empty() {
        inc_stable(&current_version, cumulative_bump)
    } else {
        escalate_stable_target(&stable_part(&current_version), cumulative_bump)
    };
    let next_n = next_prerelease_number(&current_version, &target, line_tag);
    format!("{target}-{line_tag}.{next_n}")
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

/// Re-derives the stable version a prerelease line is building toward when
/// the cumulative bump escalates. The invariant: the stable part of the
/// current prerelease already reflects the previous cumulative bump applied
/// to the version the line started from, so only an escalation changes it.
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

fn next_prerelease_number(current: &Version, target: &str, line_tag: &str) -> u64 {
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
    if current_tag != line_tag {
        return 0;
    }
    match current.pre_release.get(1) {
        Some(Identifier::Numeric(current_n)) => current_n + 1,
        _ => 0,
    }
}

fn apply_fixed_group_versions(
    participants: &BTreeMap<&str, Participant<'_>>,
    state: &BTreeMap<String, BumpState>,
    new_versions: &mut BTreeMap<String, String>,
    line_consumed_by_pkg: &BTreeMap<&str, Vec<&ChangeIntent>>,
    versioning: Option<&VersioningSettings>,
) {
    let Some(settings) = versioning else {
        return;
    };
    for group in &settings.fixed {
        let members: Vec<&str> = group
            .iter()
            .map(String::as_str)
            .filter(|name| participants.contains_key(name))
            .collect();
        let Some(group_bump) = max_bump_type_of(members.iter().filter_map(|&name| {
            state
                .get(name)
                .map(|entry| cumulative_bump_type(name, entry.bump_type, line_consumed_by_pkg))
        })) else {
            continue;
        };

        let highest_current = members
            .iter()
            .map(|&name| {
                Version::parse(participants[name].current_version)
                    .expect("participants have valid versions")
            })
            .max()
            .expect("a bumped member exists, so the group is not empty");
        let target = if highest_current.pre_release.is_empty() {
            inc_stable(&highest_current, group_bump)
        } else {
            escalate_stable_target(&stable_part(&highest_current), group_bump)
        };

        let line_tag = members.first().and_then(|name| settings.prereleases.get(*name));
        let shared_version = match line_tag {
            Some(line_tag) => {
                let next_n = members
                    .iter()
                    .map(|&name| {
                        let current = Version::parse(participants[name].current_version)
                            .expect("participants have valid versions");
                        next_prerelease_number(&current, &target, line_tag)
                    })
                    .max()
                    .unwrap_or(0);
                format!("{target}-{line_tag}.{next_n}")
            }
            None => target,
        };
        for name in members {
            if state.contains_key(name) {
                new_versions.insert(name.to_string(), shared_version.clone());
            }
        }
    }
}

/// The range that pnpm materializes for a workspace: spec at pack time, given
/// the dependency's version at the dependent's previous release. Dependent
/// propagation republishes the dependent whenever the dependency's new
/// version falls outside this range.
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
                intent.releases.get(&release.name).and_then(|bump| bump.release())
                    == Some(effective_bump)
            })
            .map(|intent| intent.file_path.display().to_string())
            .collect();
        let raised_by = if intent_files.is_empty() {
            format!(
                "constraint chain: {}",
                release.causes.iter().map(ToString::to_string).collect::<Vec<String>>().join(", ")
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

/// The bump class a release actually applies. Fixed-group version sharing and
/// prerelease-line escalation can move a version further than the package's
/// own declared or propagated bump, so the cap compares against the real
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
