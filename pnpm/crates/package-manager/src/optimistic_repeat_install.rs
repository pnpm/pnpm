//! Pre-install fast path: when nothing has changed since the last
//! install, skip the entire pipeline.
//!
//! The install logs "Already up to date" when nothing has changed,
//! before any of the install setup runs. The check keys off
//! `<workspace_root>/node_modules/.pnpm-workspace-state-v1.json`'s
//! `lastValidatedTimestamp` against each project's `package.json`
//! mtime without parsing the lockfile or touching the verifier cache or
//! resolver state. A lockfile modified after the last validation is
//! scanned with a bounded buffer for merge conflict markers before the
//! shortcut may continue.
//!
//! Scope: the mtime-vs-`lastValidatedTimestamp` branch (the
//! up-to-date exit when no project is modified), the patch-file branch
//! (a configured patch file whose mtime is newer than
//! `lastValidatedTimestamp` invalidates the fast path even when its
//! `patchedDependencies` config entry is unchanged — a content edit the
//! key→path settings comparison can't see), and the modified-manifests
//! content re-check: when a manifest's mtime is newer but its
//! dependency-relevant content still matches the lockfile, the install
//! still reports up-to-date (a `touch package.json`, a `scripts` edit, or
//! an `npm pkg set/delete` rewrite must not trigger a full install), and
//! the pnpmfile branch (an added, removed, or edited workspace pnpmfile
//! invalidates the fast path; plugin pnpmfiles from config dependencies
//! are covered by the `config_dependencies` comparison instead of the
//! mtime check), and the local-file-dependency bail: mutable directory
//! dependencies always take the full install path. Local tarballs stay on the
//! fast path only when their bytes match the integrity in the lockfile.
//! Local specs introduced through `pnpm.overrides` or package extensions
//! remain on the full path because their resolution base is graph-dependent.
//! The local-file-dependency freshness branch of linked-package
//! verification is NOT ported here. When this function returns
//! `Decision::Skipped` the caller proceeds with the full install path,
//! which has its own
//! freshness guards (`check_lockfile_freshness`, the no-op
//! short-circuit).
//!
//! ## Why a separate module
//!
//! Lives in `pnpm-package-manager` rather than a new
//! `pnpm-deps-status` crate because both consumers — `Install::run`
//! and the verify-deps-before-run gate ([`check_deps_status_before_run`])
//! — lean on install internals (`check_lockfile_settings_drift`,
//! `check_importer_satisfies`, `build_workspace_state`) that a separate
//! crate would have to re-export wholesale. Extract it only if a
//! consumer outside this crate's dependents appears.

pub(crate) mod conflict_markers;
pub(crate) mod deps_status;
pub(crate) mod local_file_deps;
pub(crate) mod manifest_agreement;
pub(crate) mod settings;
pub(crate) mod timestamps;
pub(crate) use conflict_markers::{
    LockfileConflictCheckFailure, first_lockfile_requiring_conflict_safe_install,
};
pub use deps_status::{RunDepsStatus, check_deps_status_before_run};
pub(crate) use local_file_deps::{
    has_local_file_dep_requiring_install, has_local_file_override, has_local_file_package_extension,
};
pub(crate) use manifest_agreement::{
    ManifestStat, modified_manifests_match_lockfile, stat_manifests,
};
pub(crate) use settings::{
    catalogs_cache_matches, current_settings_with_catalogs, first_setting_drift,
    recorded_supported_architectures_match, settings_match,
};
pub(crate) use timestamps::{
    FileMtime, file_mtime, file_mtime_from_metadata, filesystem_now_ms, lockfile_modified_since,
    modified_at_or_after, mtime_ms, refreshed_validation_baseline_ms, validation_baseline_ms,
    wanted_lockfile_modified,
};

mod settle;
use settle::{
    current_lockfile_file_has_content, current_lockfile_unusable_with_non_empty_wanted,
    early_repeat_verdict, first_project_missing_modules_dir, modules_dirs_present,
    project_structure_matches, settle_repeat_install,
};

use std::{
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};

use pnpm_catalogs_resolver::{CatalogResolutionResult, WantedDependency, resolve_from_catalog};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, LinkWorkspacePackages, NodeLinker, TrustPolicy};
use pnpm_lockfile::{ImporterDepVersion, Lockfile, MaybeLazyLockfile, ProjectSnapshot};
use pnpm_modules_yaml::{Host, IncludedDependencies};
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_workspace_state::{
    NodeLinker as WorkspaceStateNodeLinker, TrustPolicy as WorkspaceStateTrustPolicy,
    WorkspaceState, WorkspaceStateSettings, load_workspace_state, update_workspace_state,
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
    pub node_linker: NodeLinker,
    pub included: IncludedDependencies,
    /// The CLI-merged effective `supportedArchitectures` this run would
    /// install with (yaml plus `--cpu` / `--os` / `--libc`), compared
    /// against the recorded value like `included`.
    pub supported_architectures: Option<&'a SupportedArchitectures>,
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
    /// `<virtual_store_dir>/lock.yaml` exists, the current lockfile
    /// stands in as the wanted one — it records exactly what the
    /// previous install materialized — and `pnpm-lock.yaml` is
    /// regenerated from it before the check reports up-to-date.
    pub lockfile: MaybeLazyLockfile<'a>,
    /// Catalogs from the workspace manifest or an `updateConfig`
    /// pnpmfile hook, for resolving `catalog:` values inside
    /// `pnpm.overrides` before the lockfile settings comparison.
    pub catalogs: &'a Catalogs,
}

/// Run the workspace-state freshness fast path. Returns
/// [`Decision::UpToDate`] when the install can short-circuit.
///
/// Always returns `Decision::Skipped` when
/// `config.optimistic_repeat_install` is `false`.
#[must_use]
pub fn check_optimistic_repeat_install(check: &OptimisticRepeatInstallCheck<'_>) -> Decision {
    check_optimistic_repeat_install_ignoring(check, &[])
}

/// Run the workspace-state freshness fast path while excluding selected
/// pnpm workspace-state setting keys from the drift comparison.
pub(crate) fn check_optimistic_repeat_install_ignoring(
    check: &OptimisticRepeatInstallCheck<'_>,
    ignored_workspace_state_settings: &[&str],
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
    if let Some(reason) = state_blocks_fast_path(check, &state, ignored_workspace_state_settings) {
        return Decision::Skipped { reason };
    }
    // The fast-path conclusion: walk every manifest and report up to
    // date when none have an mtime newer than
    // `workspaceState.lastValidatedTimestamp`. The walk has to
    // succeed (read errors mean we can't *prove* freshness, so fall
    // through).
    let Some(drift) = ManifestDrift::stat(check, &state) else {
        return Decision::Skipped { reason: "failed to stat a project manifest" };
    };
    let modified = drift.modified(&state);
    if let Some(decision) = early_repeat_verdict(check, &modified, drift.lockfile_modified) {
        return decision;
    }
    // A newer mtime alone doesn't invalidate: the modified-manifests
    // branch re-checks the *content* against the wanted lockfile so a
    // rewrite that left the dependency fields intact — `touch`, a
    // `scripts` edit, `npm pkg set/delete` — still reports up to date.
    // When only the lockfile changed, every project is validated rather
    // than just the modified ones.
    let projects_to_check = drift.projects_to_check(modified);
    let filesystem_now =
        check.is_workspace_install.then(|| filesystem_now_ms(check.workspace_root)).flatten();
    match modified_manifests_match_lockfile(
        check,
        &state,
        &projects_to_check,
        check.config.dedupe_peers,
    ) {
        Ok(loaded_current) => {
            match settle_repeat_install(check, &state, loaded_current, filesystem_now) {
                Ok(()) => Decision::UpToDate,
                Err(reason) => Decision::Skipped { reason },
            }
        }
        Err(reason) => Decision::Skipped { reason },
    }
}

/// Every project manifest's mtime against the last validation, and
/// whether the wanted lockfile itself moved since.
pub(crate) struct ManifestDrift<'a> {
    stats: Vec<ManifestStat<'a>>,
    //// A lockfile-only change — `git checkout`/stash-restore of just
    //// `pnpm-lock.yaml`, or an external rewrite — leaves every manifest
    //// untouched but still invalidates the install. Probe the wanted
    //// lockfile's mtime before the manifest-mtime exit so a lockfile
    //// modification is not missed.
    pub(crate) lockfile_modified: bool,
}

impl<'a> ManifestDrift<'a> {
    /// `None` when a manifest cannot be stat'd, which leaves freshness
    /// unprovable.
    pub(crate) fn stat(
        check: &OptimisticRepeatInstallCheck<'a>,
        state: &WorkspaceState,
    ) -> Option<Self> {
        Some(Self {
            stats: stat_manifests(check.project_manifests)?,
            lockfile_modified: wanted_lockfile_modified(
                check.workspace_root,
                check.config,
                state.last_validated_timestamp,
            ),
        })
    }

    pub(crate) fn modified(&self, state: &WorkspaceState) -> Vec<&ManifestStat<'a>> {
        self.stats
            .iter()
            .filter(|stat| modified_at_or_after(stat.mtime, state.last_validated_timestamp))
            .collect()
    }

    /// The projects the content check covers: every one when the lockfile
    /// itself changed, else the modified ones.
    pub(crate) fn projects_to_check<'s>(
        &'s self,
        modified: Vec<&'s ManifestStat<'a>>,
    ) -> Vec<&'s ManifestStat<'a>> {
        if self.lockfile_modified { self.stats.iter().collect() } else { modified }
    }
}

/// The configuration alone can rule the fast path out, before anything is
/// read from disk.
fn config_blocks_fast_path(config: &Config) -> Option<&'static str> {
    if !config.optimistic_repeat_install {
        return Some("optimistic_repeat_install disabled");
    }
    // The merge has to run, and it rewrites the wanted lockfile and
    // deletes the per-branch ones — neither of which any fast path does.
    if config.merge_git_branch_lockfiles {
        return Some("the git branch lockfiles have to be merged");
    }
    None
}

/// The first reason the recorded workspace state cannot prove this install is
/// a no-op.
fn state_blocks_fast_path(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    ignored_workspace_state_settings: &[&str],
) -> Option<&'static str> {
    // A filtered install refreshes `lastValidatedTimestamp` while
    // materializing only the projects it selected, so its state cannot
    // prove anything about the rest of the workspace: an unselected
    // project's manifest edit is already older than the recorded
    // timestamp. Every install must re-validate once against a state a
    // filtered install wrote — pnpm's `ignoreFilteredInstallCache`.
    if state.filtered_install {
        return Some("the previous install was filtered");
    }
    if first_lockfile_requiring_conflict_safe_install(check, state.last_validated_timestamp)
        .is_some()
    {
        return Some("a changed lockfile contains or cannot be checked for merge conflict markers");
    }
    local_file_blocks_fast_path(check)
        .or_else(|| settings_block_fast_path(check, state, ignored_workspace_state_settings))
        .or_else(|| lockfile_inputs_block_fast_path(check, state))
}

/// A local file dependency's contents can change with nothing in the manifest
/// or the lockfile moving, so any of them rules the fast path out.
fn local_file_blocks_fast_path(check: &OptimisticRepeatInstallCheck<'_>) -> Option<&'static str> {
    let &OptimisticRepeatInstallCheck { config, included, catalogs, .. } = check;
    match has_local_file_dep_requiring_install(check) {
        Ok(true) => {
            return Some(
                "a dependency is a local file dependency and its contents may have changed",
            );
        }
        Ok(false) => {}
        Err(reason) => return Some(reason),
    }
    match has_local_file_override(config, catalogs) {
        Ok(true) => {
            return Some(
                "an override maps to a local file dependency and its contents may have changed",
            );
        }
        Err(reason) => return Some(reason),
        Ok(false) => {}
    }
    if has_local_file_package_extension(config, included, catalogs) {
        return Some(
            "a package extension injects a local file dependency and its contents may have changed",
        );
    }
    None
}

fn settings_block_fast_path(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    ignored_workspace_state_settings: &[&str],
) -> Option<&'static str> {
    let &OptimisticRepeatInstallCheck {
        config,
        node_linker,
        included,
        supported_architectures,
        project_manifests,
        catalogs,
        ..
    } = check;
    if !settings_match(
        state,
        config,
        node_linker,
        included,
        supported_architectures,
        ignored_workspace_state_settings,
    ) {
        return Some("settings drift");
    }
    if !catalogs_cache_matches(state.settings.catalogs.as_ref(), catalogs) {
        return Some("catalogs cache outdated");
    }
    if !project_structure_matches(state, project_manifests) {
        return Some("workspace project list changed");
    }
    // The "modules dir exists when the project has deps" gate: a
    // project with `dependencies`/`devDependencies` but no
    // `node_modules` cannot be up to date. The `modulesDir` is read
    // off the per-project config; pacquet doesn't track per-importer
    // overrides yet, so check the install-time `config.modules_dir`
    // for the root + `<project_root>/node_modules` for siblings,
    // matching the `isolated`-linker default.
    if !modules_dirs_present(config, node_linker, project_manifests) {
        return Some("project has dependencies but no node_modules directory");
    }
    None
}

/// The lockfile and the resolution inputs beside the manifests: a missing
/// lockfile the current one may not stand in for, an edited patch, an edited
/// pnpmfile.
fn lockfile_inputs_block_fast_path(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
) -> Option<&'static str> {
    let &OptimisticRepeatInstallCheck { workspace_root, config, is_workspace_install, .. } = check;
    // Single-project installs require a lockfile to even attempt the
    // fast path. The single-project branch raises
    // `RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND` when the wanted-lockfile
    // stat is absent, which resolves to not-up-to-date. Pacquet
    // additionally accepts the *current* lockfile
    // (`<virtual_store_dir>/lock.yaml`) as a stand-in when
    // `pnpm-lock.yaml` is missing: it records exactly what the
    // previous install materialized, so the content checks can run
    // against it and `pnpm-lock.yaml` is regenerated from it on
    // success — the same substitution the full install path makes
    // when it synthesizes the wanted lockfile from the current one.
    // Workspace installs skip this existence gate — the workspace
    // branch tolerates a missing `pnpm-lock.yaml` (the wanted-lockfile
    // scan `continue`s on ENOENT, and the missing lockfile is restored
    // from the current one rather than failing). The mtime side of that
    // probe is handled by `wanted_lockfile_modified` in the caller.
    // The current lockfile is not a stand-in for a missing *branch*
    // lockfile: it records what the previous branch's install
    // materialized, and pnpm refuses the substitution for the same
    // reason.
    if config.use_git_branch_lockfile
        && !workspace_root.join(config.wanted_lockfile_name()).exists()
    {
        return Some("the branch lockfile is missing");
    }
    if !is_workspace_install
        && !workspace_root.join(config.wanted_lockfile_name()).exists()
        && !current_lockfile_file_has_content(&config.virtual_store_dir)
    {
        return Some("wanted lockfile missing");
    }
    // A patch file edited in place keeps the same `patchedDependencies`
    // key→path entry (so `settings_match` can't see the change) but
    // changes the patched output and the patch hash. This check runs
    // before the manifest-modified exit so the patch reason wins when
    // both a patch and a manifest are newer than the last validation.
    if patches_modified_since(workspace_root, config, state.last_validated_timestamp) {
        return Some("a patch file is newer than the last validation");
    }
    // A pnpmfile added, removed, or edited in place can change
    // resolution (readPackage rewrites, custom resolvers, a
    // `shouldRefreshResolution` verdict) without touching any manifest,
    // so it must defeat the mtime fast path.
    if pnpmfiles_modified_since(
        workspace_root,
        config,
        &state.pnpmfiles,
        state.last_validated_timestamp,
    ) {
        return Some("a pnpmfile changed since the last validation");
    }
    None
}

fn manifest_has_runtime_deps(manifest: &PackageManifest) -> bool {
    let value = manifest.value();
    [value.get("dependencies"), value.get("devDependencies"), value.get("optionalDependencies")]
        .into_iter()
        .flatten()
        .any(|deps| deps.as_object().is_some_and(|map| !map.is_empty()))
}

fn manifest_string_field(manifest: &PackageManifest, key: &str) -> Option<String> {
    manifest.value().get(key).and_then(|v| v.as_str()).map(ToString::to_string)
}

/// Whether any configured patch file's mtime is newer than the last
/// validation. A patch that can't be stat'd is treated as not-modified,
/// leaving a genuinely missing patch to surface on the full install
/// path. Patch paths are resolved against `workspace_root` (the
/// `pnpm-workspace.yaml` dir, where `patchedDependencies` is declared),
/// matching how [`Config::patched_dependency_hashes`] resolves them.
fn patches_modified_since(workspace_root: &Path, config: &Config, cutoff_ms: i64) -> bool {
    let Some(patches) = config.patched_dependencies.as_ref() else {
        return false;
    };
    patches.values().any(|rel_or_abs| {
        let candidate = Path::new(rel_or_abs);
        let path = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            workspace_root.join(candidate)
        };
        file_mtime(&path).is_some_and(|mtime| modified_at_or_after(mtime, cutoff_ms))
    })
}

/// The pnpmfile list recorded in the workspace state and compared by
/// the freshness check: today just the workspace pnpmfile.
/// Config-dependency plugin pnpmfiles are tracked via the
/// `config_dependencies` comparison instead. An install that ignores
/// the pnpmfile records none, so the next install that honors it again
/// sees the list change and re-validates.
pub(crate) fn current_pnpmfiles(workspace_root: &Path, config: &Config) -> Vec<String> {
    if config.ignore_pnpmfile {
        return Vec::new();
    }
    pnpm_hooks::finder::find_pnpmfiles(workspace_root, crate::pnpmfile_selection(config))
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// Whether the pnpmfiles changed since the last validation: the
/// recorded pnpmfile list must match the current one, every recorded
/// pnpmfile must still exist, and none may be newer than the last
/// validation.
fn pnpmfiles_modified_since(
    workspace_root: &Path,
    config: &Config,
    previous: &[String],
    cutoff_ms: i64,
) -> bool {
    pnpmfiles_drift(workspace_root, config, previous, cutoff_ms).is_some()
}

/// [`pnpmfiles_modified_since`] with the drift spelled out in pnpm's
/// issue wording, for the verify-deps-before-run gate's user-facing
/// messages.
fn pnpmfiles_drift(
    workspace_root: &Path,
    config: &Config,
    previous: &[String],
    cutoff_ms: i64,
) -> Option<String> {
    let current = current_pnpmfiles(workspace_root, config);
    if current != previous {
        return Some("The list of pnpmfiles changed.".to_string());
    }
    current.iter().find_map(|path| {
        let Some(mtime) = file_mtime(Path::new(path)) else {
            return Some(format!(r#"pnpmfile at "{path}" was removed"#));
        };
        modified_at_or_after(mtime, cutoff_ms)
            .then(|| format!(r#"pnpmfile at "{path}" was modified"#))
    })
}

#[cfg(test)]
mod tests;
