//! Pre-install fast path: when nothing has changed since the last
//! install, skip the entire pipeline.
//!
//! The install logs "Already up to date" when nothing has changed,
//! before any of the install setup runs. The check compares the content
//! digests [`validation`] recorded in
//! `<workspace_root>/node_modules/.pnpm-workspace-state-v1.json` with the
//! manifests and the lockfile, without parsing the lockfile or touching the
//! verifier cache or resolver state while every project is still proven
//! installed. A lockfile modified after the last validation is scanned with
//! a bounded buffer for merge conflict markers before the shortcut may
//! continue.

pub(crate) mod conflict_markers;
pub(crate) mod deps_status;
pub(crate) mod freshness;
pub(crate) mod local_file_deps;
pub(crate) mod manifest_agreement;
pub(crate) mod settings;
pub(crate) mod timestamps;
pub(crate) mod validation;
pub(crate) use conflict_markers::{
    LockfileConflictCheckFailure, first_lockfile_requiring_conflict_safe_install,
};
pub(crate) use current_lockfile::materialized_shape_matches;
pub use deps_status::{RunDepsStatus, check_deps_status_before_run};
pub(crate) use freshness::Scope;
pub(crate) use local_file_deps::{
    FrozenLocalTarballCheck, frozen_local_tarballs_to_verify, has_local_file_dep_requiring_install,
    has_local_file_override, has_local_file_package_extension,
};
pub(crate) use manifest_agreement::{ManifestStat, held_projects_match_lockfile};
pub use manifest_source::RepeatInstallManifests;
pub(crate) use modules_dirs::{hoisted_project_modules_dir, project_id};
pub(crate) use relocation::recorded_elsewhere;
pub(crate) use settings::{
    catalogs_cache_matches, current_settings_with_catalogs, first_setting_drift,
    recorded_supported_architectures_match,
};
pub(crate) use timestamps::{
    file_mtime, file_mtime_from_metadata, filesystem_now_ms_in_modules,
    filesystem_now_ns_in_modules, last_validated_timestamp, lockfile_modified_since,
    modified_at_or_after,
};

mod current_lockfile;
mod manifest_source;
mod modules_dirs;
mod relocation;
mod settle;
use freshness::{
    Gate, GatePolicy, MissingWantedLockfile, NewerPatch, RecordedGroups, Stale, check_freshness,
};
use modules_dirs::{
    SelectedReach, direct_dependency_link_dangling, first_project_missing_modules_dir,
    first_selected_project_missing_modules_dir,
};
use settle::{
    current_lockfile_file_has_content, project_structure_matches, wanted_lockfile_file_has_content,
};

use std::{
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};

use pnpm_catalogs_resolver::{
    CatalogAnchor, CatalogResolutionResult, WantedDependency, resolve_from_catalog,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, LinkWorkspacePackages, NodeLinker, TrustPolicy};
use pnpm_lockfile::{ImporterDepVersion, Lockfile, MaybeLazyLockfile, ProjectSnapshot};
use pnpm_modules_yaml::{Host, IncludedDependencies};
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_workspace_state::{
    NodeLinker as WorkspaceStateNodeLinker, TrustPolicy as WorkspaceStateTrustPolicy,
    WorkspaceState, WorkspaceStateSettings, load_workspace_state,
};

/// Outcome of [`check_optimistic_repeat_install`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The install is fully up to date — emit "Already up to date"
    /// and exit before any of the install setup runs.
    UpToDate,
    /// Fall through to the full install path. `reason` is a short
    /// diagnostic string surfaced via `tracing::debug!` for
    /// diagnosability without contaminating the reporter stream.
    Skipped { reason: &'static str },
}

/// Inputs to [`check_optimistic_repeat_install`].
pub struct OptimisticRepeatInstallCheck<'a> {
    /// The root the install recorded its lockfile and workspace state
    /// against, which importer ids and relative local `pnpm.overrides`
    /// targets are named from too. The directory containing
    /// `pnpm-workspace.yaml` (or the project root when no workspace
    /// manifest exists — same fallback as
    /// [`Install::run`](crate::Install::run)), unless the configuration
    /// pins the lockfile somewhere else.
    pub workspace_root: &'a Path,
    pub config: &'a Config,
    /// Every importer's `(root_dir, manifest)` pair. For a
    /// single-project install it's just the root manifest; for a
    /// workspace install it's every project the resolver would
    /// otherwise walk. The caller passes this in (rather than this
    /// function rediscovering it) so the same walk seeds the regular
    /// install path on the fall-through.
    pub project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    /// `true` when a `pnpm-workspace.yaml` drives the install — that
    /// selects the workspace branch, which keys the manifest and
    /// lockfile comparisons off `lastValidatedTimestamp`. `false` (no
    /// workspace manifest) selects the single-project branch, which
    /// additionally requires `pnpm-lock.yaml` to exist on disk —
    /// `RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND` is raised otherwise, which
    /// resolves to not-up-to-date — and keys its comparisons off the
    /// lockfile mtimes instead.
    pub is_workspace_install: bool,
    /// The wanted lockfile (`None` once loaded when `pnpm-lock.yaml`
    /// is absent or empty). Consulted only by the modified-manifests
    /// content re-check; the pure-mtime fast path never parses it —
    /// which is why it arrives lazily, so the common repeat-install
    /// run skips the YAML parse entirely. A separately bounded byte
    /// scan only runs when lockfile metadata changed. When absent and
    /// `<install_state_dir>/lock.yaml` exists, the current lockfile
    /// stands in as the wanted one — it records exactly what the
    /// previous install materialized — and `pnpm-lock.yaml` is
    /// regenerated from it before the check reports up-to-date.
    pub lockfile: MaybeLazyLockfile<'a>,
    /// Catalogs from the workspace manifest or an `updateConfig`
    /// pnpmfile hook, for resolving `catalog:` values inside
    /// `pnpm.overrides` before the lockfile settings comparison.
    pub catalogs: &'a Catalogs,
    pub layout: RepeatInstallLayout<'a>,
    pub manifests: RepeatInstallManifests<'a>,
}

#[derive(Clone, Copy)]
pub struct RepeatInstallLayout<'a> {
    pub node_linker: NodeLinker,
    pub included: IncludedDependencies,
    /// The CLI-merged effective `supportedArchitectures` this run would
    /// install with (yaml plus `--cpu` / `--os` / `--libc`), compared
    /// against the recorded value like `included`.
    pub supported_architectures: Option<&'a SupportedArchitectures>,
}

/// Run the workspace-state freshness fast path. Returns
/// [`Decision::UpToDate`] when the install can short-circuit.
///
/// Always returns `Decision::Skipped` when
/// `config.optimistic_repeat_install` is `false`.
#[must_use]
pub fn check_optimistic_repeat_install(check: &OptimisticRepeatInstallCheck<'_>) -> Decision {
    check_optimistic_repeat_install_in(check, Scope::Workspace)
}

/// [`check_optimistic_repeat_install`] for an install covering `scope`.
pub(crate) fn check_optimistic_repeat_install_in(
    check: &OptimisticRepeatInstallCheck<'_>,
    scope: Scope<'_>,
) -> Decision {
    if let Some(reason) = config_blocks_fast_path(check.config) {
        return Decision::Skipped { reason };
    }
    // No workspace state means no previous install has completed
    // (or the file was deleted) — there's no `lastValidatedTimestamp`
    // to compare against.
    let Ok(Some(state)) = load_workspace_state(check.workspace_root) else {
        return Decision::Skipped { reason: "no workspace state on disk" };
    };
    check_optimistic_repeat_install_with_state(check, state, scope)
}

pub(crate) fn check_optimistic_repeat_install_with_state(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: WorkspaceState,
    scope: Scope<'_>,
) -> Decision {
    let (state, moved) =
        relocation::relocated_state(&state, check.workspace_root, check.project_manifests)
            .map_or((state, false), |relocated| (relocated, true));
    let policy = GatePolicy {
        gate: Gate::Install,
        ignored_settings: &[],
        check_local_file_deps: true,
        check_dangling_direct_links: true,
        missing_wanted_lockfile: MissingWantedLockfile::Restore,
        newer_patch: NewerPatch::Refuse,
        dedupe_peers: check.config.dedupe_peers,
        recorded_groups: RecordedGroups::Current,
    };
    match check_freshness(check, &state, moved, &policy, scope) {
        Ok(()) => Decision::UpToDate,
        Err(stale) => Decision::Skipped { reason: skip_reason(&stale) },
    }
}

/// The diagnostic reason the fast path is skipped for.
fn skip_reason(stale: &Stale) -> &'static str {
    match stale {
        Stale::UnrecordedFilteredState => "the previous install was filtered",
        Stale::LockfileConflict { .. } => {
            "a changed lockfile contains or cannot be checked for merge conflict markers"
        }
        Stale::LocalFileDep(reason) | Stale::MoveRefused(reason) | Stale::Content(reason) => reason,
        Stale::SettingChanged(_) => "settings drift",
        Stale::ConfigDependencies => "config dependencies drift",
        Stale::CatalogsCache => "catalogs cache outdated",
        Stale::ProjectStructure => "workspace project list changed",
        Stale::MissingModulesDir(_) => "project has dependencies but no node_modules directory",
        Stale::DanglingDirectLink => "a direct dependency link points to a missing target",
        Stale::MissingBranchLockfile => "the branch lockfile is missing",
        Stale::Pnpmfile(_) => "a pnpmfile changed since the last validation",
        Stale::Unprovable => "the lockfile cannot be read",
        Stale::NothingInstalled => "current lockfile missing",
        Stale::ProjectNotInstalled(_) => "a project with dependencies was never installed",
        Stale::NoLockfile => "wanted lockfile missing",
    }
}

/// The configuration alone can rule the fast path out, before anything is
/// read from disk.
pub(crate) fn config_blocks_fast_path(config: &Config) -> Option<&'static str> {
    if config.node_linker == pnpm_config::NodeLinker::Loaded {
        return Some("CAS installation requires manifest verification");
    }
    if !config.optimistic_repeat_install {
        return Some("optimistic_repeat_install disabled");
    }
    if config.prune_direct_dependencies {
        return Some("prune_direct_dependencies enabled");
    }
    // The merge has to run, and it rewrites the wanted lockfile and
    // deletes the per-branch ones — neither of which any fast path does.
    if config.merge_git_branch_lockfiles {
        return Some("the git branch lockfiles have to be merged");
    }
    None
}

/// A local file dependency's contents can change with nothing in the manifest
/// or the lockfile moving, so any of them rules the fast path out.
fn local_file_blocks_fast_path(check: &OptimisticRepeatInstallCheck<'_>) -> Option<&'static str> {
    let &OptimisticRepeatInstallCheck {
        config,
        catalogs,
        layout: crate::RepeatInstallLayout { included, .. },
        ..
    } = check;
    let Ok(parsed_overrides) = crate::install::parse_config_overrides(config, catalogs) else {
        return Some("pnpm.overrides cannot be parsed");
    };
    let overrides = parsed_overrides
        .as_deref()
        .unwrap_or(&[]);
    match has_local_file_dep_requiring_install(check, overrides) {
        Ok(true) => {
            return Some(
                "a dependency is a local file dependency and its contents may have changed",
            );
        }
        Ok(false) => {}
        Err(reason) => return Some(reason),
    }
    if has_local_file_override(overrides) {
        return Some(
            "an override maps to a local file dependency and its contents may have changed",
        );
    }
    if has_local_file_package_extension(config, included, catalogs, overrides) {
        return Some(
            "a package extension injects a local file dependency and its contents may have changed",
        );
    }
    None
}

pub(crate) fn manifest_has_runtime_deps(manifest: &PackageManifest) -> bool {
    let value = manifest.value();
    [value.get("dependencies"), value.get("devDependencies"), value.get("optionalDependencies")]
        .into_iter()
        .flatten()
        .any(|deps| {
            deps.as_object()
                .is_some_and(|map| !map.is_empty())
        })
}

/// Whether the manifest declares at least one peer that is not marked
/// optional. The caller decides whether `autoInstallPeers` fetches them.
pub(crate) fn manifest_has_required_peers(manifest: &PackageManifest) -> bool {
    let Some(peers) = manifest
        .value()
        .get("peerDependencies")
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    let meta = manifest
        .value()
        .get("peerDependenciesMeta")
        .and_then(serde_json::Value::as_object);
    peers
        .keys()
        .any(|name| !peer_dependency_is_optional(meta, name))
}

fn peer_dependency_is_optional(
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
    name: &str,
) -> bool {
    meta.and_then(|meta| meta.get(name))
        .and_then(serde_json::Value::as_object)
        .and_then(|entry| entry.get("optional"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

fn manifest_string_field(manifest: &PackageManifest, key: &str) -> Option<String> {
    manifest
        .value()
        .get(key)
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

/// Whether the file at `path` may have changed at or after `cutoff_ms`. Only
/// a state without validation records, which has no content digests, is
/// judged this way. A file that can't be stat'd is treated as not modified, leaving a
/// genuinely missing one to surface on the full install path.
fn patch_modified_since(path: &Path, cutoff_ms: i64) -> bool {
    file_mtime(path).is_some_and(|mtime| modified_at_or_after(mtime, cutoff_ms))
}

/// The first project `scope` holds that needs a modules directory and lacks
/// one. A state that leaves some project unproven, such as one a filtered
/// install wrote, holds only the selected projects to it.
fn first_held_project_missing_modules_dir(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    scope: Scope<'_>,
) -> Option<String> {
    match scope {
        Scope::Selected(selected) if state.filtered_install => {
            first_selected_project_missing_modules_dir(
                check,
                state,
                selected,
                SelectedReach::SelectedOnly,
            )
        }
        Scope::Command(selected) if state.filtered_install => {
            first_selected_project_missing_modules_dir(
                check,
                state,
                selected,
                SelectedReach::WithWorkspaceDeps,
            )
        }
        _ => first_project_missing_modules_dir(check, state),
    }
}

/// The pnpmfile list recorded in the workspace state and compared by
/// the freshness check: every pnpmfile the install loads. An install
/// that ignores the pnpmfile records none, so the next install that
/// honors it again sees the list change and re-validates.
pub(crate) fn current_pnpmfiles(workspace_root: &Path, config: &Config) -> Vec<String> {
    if config.ignore_pnpmfile {
        return Vec::new();
    }
    crate::find_install_pnpmfiles(config, workspace_root)
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// How the pnpmfiles changed since the last validation, in pnpm's issue
/// wording: the recorded list must match the current one, and every recorded
/// pnpmfile must still exist with the content it was validated with.
fn pnpmfiles_drift(
    workspace_root: &Path,
    config: &Config,
    state: &WorkspaceState,
) -> Option<String> {
    let current = current_pnpmfiles(workspace_root, config);
    if current != state.pnpmfiles {
        return Some("The list of pnpmfiles changed.".to_string());
    }
    let recorded = state.validation.as_ref();
    let digests = validation::FileDigests::new(recorded);
    current
        .iter()
        .find_map(|path| {
            let removed = || format!(r#"pnpmfile at "{path}" was removed"#);
            let modified = || format!(r#"pnpmfile at "{path}" was modified"#);
            let then = recorded.and_then(|recorded| recorded.files.get(path));
            let Some(then) = then else {
                // A state without validation records has no digests.
                let mtime = file_mtime(Path::new(path)).map_or_else(|| Err(removed()), Ok);
                return match mtime {
                    Err(removed) => Some(removed),
                    Ok(mtime) => {
                        modified_at_or_after(mtime, state.last_validated_timestamp).then(modified)
                    }
                };
            };
            match digests.of(Path::new(path)) {
                validation::FileDigest::Absent => Some(removed()),
                validation::FileDigest::Unreadable => Some(modified()),
                validation::FileDigest::Present(now) => (then.digest != now.digest).then(modified),
            }
        })
}

#[cfg(test)]
mod tests;
