pub use project_refs::{ProjectRefIndex, index_project_refs, is_dir_ref, to_project_dir};
pub use versions::materialize_workspace_range;

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

pub fn assemble_release_plan(
    projects: &[WorkspaceProject],
    workspace_dir: &Path,
    intents: &[ChangeIntent],
    ledger: &Ledger,
    versioning: Option<&VersioningSettings>,
    opts: &AssembleReleasePlanOptions,
) -> Result<ReleasePlan, VersioningError> {
    let workspace = resolve_workspace(projects, workspace_dir, versioning)?;
    let intent_bumps = resolve_intents(intents, &workspace.refs, &workspace.participants)?;
    if opts.enforce_workspace_protocol {
        assert_internal_deps_use_workspace_protocol(&workspace.participants)?;
    }
    let consumption = build_consumption_index(ledger, |name| workspace.refs.name_to_dirs(name))?;

    let ctx = AssembleContext {
        participants: &workspace.participants,
        lanes_by_dir: &workspace.lanes_by_dir,
        fixed_groups: &workspace.fixed_groups,
        epics: &workspace.epics,
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

/// The workspace's release structure the plan and the invariant check share:
/// its projects, lanes, fixed groups and epics, validated against each other.
struct ResolvedWorkspace<'a> {
    refs: ProjectRefIndex,
    participants: BTreeMap<String, Participant<'a>>,
    lanes_by_dir: BTreeMap<String, String>,
    fixed_groups: Vec<Vec<String>>,
    epics: Vec<ResolvedEpic>,
}

fn resolve_workspace<'a>(
    projects: &'a [WorkspaceProject],
    workspace_dir: &Path,
    versioning: Option<&VersioningSettings>,
) -> Result<ResolvedWorkspace<'a>, VersioningError> {
    let refs = index_project_refs(projects, workspace_dir);
    let participants = collect_participants(projects, workspace_dir, &refs, versioning)?;
    let lanes_by_dir = resolve_lanes(&refs, versioning)?;
    let fixed_groups = resolve_fixed_groups(&refs, &participants, versioning)?;
    validate_fixed_group_lanes(&fixed_groups, &lanes_by_dir, versioning)?;
    let epics = resolve_epics(&refs, &participants, versioning)?;
    validate_epics(&epics, &fixed_groups)?;
    Ok(ResolvedWorkspace { refs, participants, lanes_by_dir, fixed_groups, epics })
}

/// The kind of committed-version invariant [`check_versioning_invariants`]
/// found broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersioningInvariantCode {
    /// An epic member's major is outside the band selected by its lead.
    EpicOutOfBand,
    /// The members of a fixed group do not share one version.
    FixedGroupMismatch,
}

/// A committed-version invariant the `versioning` configuration declares that
/// the workspace currently violates.
#[derive(Debug, Clone)]
pub struct VersioningInvariantViolation {
    /// The machine-readable kind of invariant that failed.
    pub code: VersioningInvariantCode,
    /// A human-readable description of the packages and versions involved.
    pub message: String,
}

/// Checks committed versions against the epic bands and fixed groups that the
/// versioning configuration declares.
///
/// Valid configuration returns `Ok` containing every
/// [`VersioningInvariantViolation`], including an empty vector when all
/// invariants hold. Invalid configuration returns `Err` containing a
/// [`VersioningError`] before committed versions are checked.
pub fn check_versioning_invariants(
    projects: &[WorkspaceProject],
    workspace_dir: &Path,
    versioning: Option<&VersioningSettings>,
) -> Result<Vec<VersioningInvariantViolation>, VersioningError> {
    let workspace = resolve_workspace(projects, workspace_dir, versioning)?;

    let mut violations = Vec::new();
    push_epic_band_violations(&workspace, &mut violations);
    push_fixed_group_violations(&workspace, versioning, &mut violations);
    Ok(violations)
}

/// With no plan (no new versions), the band derives from the lead's current
/// major, and members are checked against their current versions.
fn push_epic_band_violations(
    workspace: &ResolvedWorkspace<'_>,
    violations: &mut Vec<VersioningInvariantViolation>,
) {
    let empty_new_versions = BTreeMap::new();
    for epic in &workspace.epics {
        let band = epic_band(epic, &workspace.participants, &empty_new_versions);
        let mut member_dirs: Vec<&String> = epic.member_dirs.iter().collect();
        member_dirs.sort();
        for member_dir in member_dirs {
            let member = &workspace.participants[member_dir.as_str()];
            let member_major = Version::parse(member.current_version)
                .expect("participants have valid versions")
                .major;
            if !band.contains(member_major) {
                violations.push(VersioningInvariantViolation {
                    code: VersioningInvariantCode::EpicOutOfBand,
                    message: format!(
                        r#"{} is at {}, whose major {member_major} is outside the band {}-{} of the epic led by "{}" (major {})."#,
                        member.name, member.current_version, band.low, band.high, epic.lead_ref, band.major,
                    ),
                });
            }
        }
    }
}

fn push_fixed_group_violations(
    workspace: &ResolvedWorkspace<'_>,
    versioning: Option<&VersioningSettings>,
    violations: &mut Vec<VersioningInvariantViolation>,
) {
    for (index, group) in workspace.fixed_groups.iter().enumerate() {
        let distinct: BTreeSet<&str> =
            group.iter().map(|dir| workspace.participants[dir.as_str()].current_version).collect();
        if distinct.len() > 1 {
            let detail = group
                .iter()
                .map(|dir| {
                    let member = &workspace.participants[dir.as_str()];
                    format!("{}@{}", member.name, member.current_version)
                })
                .collect::<Vec<_>>()
                .join(", ");
            let declared =
                versioning.map(|settings| settings.fixed[index].join(", ")).unwrap_or_default();
            violations.push(VersioningInvariantViolation {
                code: VersioningInvariantCode::FixedGroupMismatch,
                message: format!("The fixed group [{declared}] is not in lockstep: {detail}."),
            });
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
    let intents = PlanIntents {
        pending_by_dir: collect_pending_intents(ctx),
        lane_consumed_by_dir: collect_lane_consumed_intents(ctx),
    };

    let mut state: BTreeMap<String, BumpState> = BTreeMap::new();
    seed_bumps(ctx, &intents, selection, &mut state);

    let mut new_versions: BTreeMap<String, String> = BTreeMap::new();
    loop {
        compute_versions(ctx, &intents, &state, &mut new_versions);
        if !propagate_bumps(ctx, &new_versions, &mut state) {
            break;
        }
    }

    let releases = planned_releases(ctx, &intents, &state, &new_versions);
    assert_no_duplicate_release_identity(&releases)?;
    if ctx.opts.snapshot_suffix.is_none() {
        enforce_epic_bands(ctx.epics, ctx.participants, &new_versions)?;
        enforce_max_bump(&releases, ctx.versioning)?;
    }

    Ok(ReleasePlan { releases })
}

#[cfg(test)]
mod tests;

mod project_refs;

use project_refs::{
    assert_internal_deps_use_workspace_protocol, collect_participants, parse_workspace_spec_alias,
    resolve_config_ref,
};

mod configuration;
use configuration::{
    resolve_epics, resolve_fixed_groups, resolve_intents, resolve_lanes, validate_epics,
    validate_fixed_group_lanes,
};

mod versions;
use versions::{
    apply_epic_band_versions, apply_fixed_group_versions, bump_release_order, compute_new_version,
    enforce_epic_bands, enforce_max_bump, epic_band, epic_rebase_floor, max_bump_type,
    max_bump_type_of, range_accepts,
};

mod propagation;
use propagation::{
    PlanIntents, assert_no_duplicate_release_identity, collect_lane_consumed_intents,
    collect_pending_intents, compute_versions, planned_releases, propagate_bumps, seed_bumps,
};
