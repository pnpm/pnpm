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
//! mtime check), and the local-file-dependency bail: no tracked mtime
//! covers the *contents* of a local file dependency (a `file:` specifier
//! or a bare local path/tarball spec, declared directly or through a
//! `pnpm.overrides` entry), so projects declaring one always take the
//! full install path, which refetches those dependencies. The
//! local-file-dependency freshness branch of linked-package verification
//! is NOT ported here. When this function returns `Decision::Skipped` the
//! caller proceeds with the full install path, which still has its own
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
    has_local_file_dep, has_local_file_override, has_local_file_package_extension,
};
pub(crate) use manifest_agreement::{
    LinkedPackagesContext, ManifestStat, modified_manifests_match_lockfile, stat_manifests,
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
    /// The directory containing `pnpm-workspace.yaml` (or the project
    /// root when no workspace manifest exists — same fallback as
    /// [`Install::run`](crate::Install::run)).
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
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        node_linker,
        included,
        supported_architectures,
        project_manifests,
        is_workspace_install,
        catalogs,
        ..
    } = check;
    if !config.optimistic_repeat_install {
        return Decision::Skipped { reason: "optimistic_repeat_install disabled" };
    }

    // The merge has to run, and it rewrites the wanted lockfile and
    // deletes the per-branch ones — neither of which any fast path does.
    if config.merge_git_branch_lockfiles {
        return Decision::Skipped { reason: "the git branch lockfiles have to be merged" };
    }

    // No workspace state means no previous install has completed
    // (or the file was deleted) — there's no `lastValidatedTimestamp`
    // to compare against.
    let Ok(Some(state)) = load_workspace_state(workspace_root) else {
        return Decision::Skipped { reason: "no workspace state on disk" };
    };

    // A filtered install refreshes `lastValidatedTimestamp` while
    // materializing only the projects it selected, so its state cannot
    // prove anything about the rest of the workspace: an unselected
    // project's manifest edit is already older than the recorded
    // timestamp. Every install must re-validate once against a state a
    // filtered install wrote — pnpm's `ignoreFilteredInstallCache`.
    if state.filtered_install {
        return Decision::Skipped { reason: "the previous install was filtered" };
    }

    if first_lockfile_requiring_conflict_safe_install(check, state.last_validated_timestamp)
        .is_some()
    {
        return Decision::Skipped {
            reason: "a changed lockfile contains or cannot be checked for merge conflict markers",
        };
    }

    // Unconditional here because the only caller is the install
    // command, which always treats local file deps as outdated.
    if has_local_file_dep(project_manifests, included, catalogs) {
        return Decision::Skipped {
            reason: "a dependency is a local file dependency and its contents may have changed",
        };
    }
    match has_local_file_override(config, catalogs) {
        Ok(true) => {
            return Decision::Skipped {
                reason: "an override maps to a local file dependency and its contents may have changed",
            };
        }
        Err(reason) => return Decision::Skipped { reason },
        Ok(false) => {}
    }
    if has_local_file_package_extension(config, included, catalogs) {
        return Decision::Skipped {
            reason: "a package extension injects a local file dependency and its contents may have changed",
        };
    }

    if !settings_match(
        &state,
        config,
        node_linker,
        included,
        supported_architectures,
        ignored_workspace_state_settings,
    ) {
        return Decision::Skipped { reason: "settings drift" };
    }

    if !catalogs_cache_matches(state.settings.catalogs.as_ref(), catalogs) {
        return Decision::Skipped { reason: "catalogs cache outdated" };
    }

    if !project_structure_matches(&state, project_manifests) {
        return Decision::Skipped { reason: "workspace project list changed" };
    }

    // The "modules dir exists when the project has deps" gate: a
    // project with `dependencies`/`devDependencies` but no
    // `node_modules` cannot be up to date. The `modulesDir` is read
    // off the per-project config; pacquet doesn't track per-importer
    // overrides yet, so check the install-time `config.modules_dir`
    // for the root + `<project_root>/node_modules` for siblings,
    // matching the `isolated`-linker default.
    if !modules_dirs_present(config, node_linker, project_manifests) {
        return Decision::Skipped {
            reason: "project has dependencies but no node_modules directory",
        };
    }

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
    // probe is handled by `wanted_lockfile_modified` below.
    // The current lockfile is not a stand-in for a missing *branch*
    // lockfile: it records what the previous branch's install
    // materialized, and pnpm refuses the substitution for the same
    // reason.
    if config.use_git_branch_lockfile
        && !workspace_root.join(config.wanted_lockfile_name()).exists()
    {
        return Decision::Skipped { reason: "the branch lockfile is missing" };
    }
    if !is_workspace_install
        && !workspace_root.join(config.wanted_lockfile_name()).exists()
        && !current_lockfile_file_has_content(&config.virtual_store_dir)
    {
        return Decision::Skipped { reason: "wanted lockfile missing" };
    }

    // A patch file edited in place keeps the same `patchedDependencies`
    // key→path entry (so `settings_match` can't see the change) but
    // changes the patched output and the patch hash. This check runs
    // before the manifest-modified exit so the patch reason wins when
    // both a patch and a manifest are newer than the last validation.
    if patches_modified_since(workspace_root, config, state.last_validated_timestamp) {
        return Decision::Skipped { reason: "a patch file is newer than the last validation" };
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
        return Decision::Skipped { reason: "a pnpmfile changed since the last validation" };
    }

    // The fast-path conclusion: walk every manifest and report up to
    // date when none have an mtime newer than
    // `workspaceState.lastValidatedTimestamp`. The walk has to
    // succeed (read errors mean we can't *prove* freshness, so fall
    // through).
    let Some(manifest_stats) = stat_manifests(project_manifests) else {
        return Decision::Skipped { reason: "failed to stat a project manifest" };
    };
    let modified: Vec<&ManifestStat<'_>> = manifest_stats
        .iter()
        .filter(|stat| modified_at_or_after(stat.mtime, state.last_validated_timestamp))
        .collect();

    // A lockfile-only change — `git checkout`/stash-restore of just
    // `pnpm-lock.yaml`, or an external rewrite — leaves every manifest
    // untouched but still invalidates the install. Probe the wanted
    // lockfile's mtime before the manifest-mtime exit so a lockfile
    // modification is not missed.
    let lockfile_modified =
        wanted_lockfile_modified(workspace_root, config, state.last_validated_timestamp);

    match current_lockfile_unusable_with_non_empty_wanted(check) {
        Ok(true) => return Decision::Skipped { reason: "current lockfile missing" },
        Ok(false) => {}
        Err(reason) => return Decision::Skipped { reason },
    }

    if modified.is_empty() && !lockfile_modified {
        return match regenerate_wanted_lockfile_if_missing(check, None) {
            Ok(()) => Decision::UpToDate,
            Err(reason) => Decision::Skipped { reason },
        };
    }

    // A newer mtime alone doesn't invalidate: the modified-manifests
    // branch re-checks the *content* against the wanted lockfile so a
    // rewrite that left the dependency fields intact — `touch`, a
    // `scripts` edit, `npm pkg set/delete` — still reports up to date.
    // When only the lockfile changed, every project is validated rather
    // than just the modified ones.
    let projects_to_check: Vec<&ManifestStat<'_>> =
        if lockfile_modified { manifest_stats.iter().collect() } else { modified };
    let filesystem_now =
        if is_workspace_install { filesystem_now_ms(workspace_root) } else { None };
    match modified_manifests_match_lockfile(check, &state, &projects_to_check, config.dedupe_peers)
    {
        Ok(loaded_current) => {
            if let Err(reason) = regenerate_wanted_lockfile_if_missing(check, loaded_current) {
                return Decision::Skipped { reason };
            }
            // Update `lastValidatedTimestamp` to prevent a pointless
            // repeat: the workspace branch rewrites the state after the
            // content checks pass. The single-project branch keys its
            // comparisons off the lockfile mtimes instead and leaves the
            // state alone. A failed write only costs the next run a
            // repeat of the content check, so it degrades rather than
            // fails.
            if is_workspace_install {
                // This path refreshes the timestamp without materializing
                // anything, so it carries the previous run's
                // `filtered_install` forward: clearing it would claim every
                // importer is materialized when a filtered install left the
                // unselected ones untouched.
                let mut new_state = crate::install::build_workspace_state::<Host>(
                    workspace_root,
                    config,
                    node_linker,
                    included,
                    supported_architectures,
                    catalogs,
                    project_manifests,
                    state.filtered_install,
                );
                new_state.last_validated_timestamp = refreshed_validation_baseline_ms(
                    new_state.last_validated_timestamp,
                    filesystem_now,
                );
                if let Err(error) = update_workspace_state(workspace_root, &new_state) {
                    tracing::warn!(
                        target: "pacquet::install",
                        ?error,
                        "Failed to refresh the workspace state after the repeat-install content check",
                    );
                }
            }
            Decision::UpToDate
        }
        Err(reason) => Decision::Skipped { reason },
    }
}

/// Restore a missing `pnpm-lock.yaml` from the current lockfile before
/// the fast path reports "Already up to date", so the short-circuit
/// leaves the same on-disk contract a full install would (the full
/// path synthesizes the wanted lockfile from the current one and
/// rewrites it). No-op when `pnpm-lock.yaml` was loaded, when lockfile
/// writing is disabled (`lockfile: false`), or when there is no
/// current lockfile to restore from (a dependency-less project).
/// A write failure falls through to the full install path rather than
/// reporting up-to-date while leaving the lockfile missing.
fn regenerate_wanted_lockfile_if_missing(
    check: &OptimisticRepeatInstallCheck<'_>,
    loaded_current: Option<Lockfile>,
) -> Result<(), &'static str> {
    if check.lockfile.is_loaded_or_on_disk() || !check.config.lockfile {
        return Ok(());
    }
    let current = match loaded_current {
        Some(current) => Some(current),
        None => Lockfile::load_current_from_virtual_store_dir(&check.config.virtual_store_dir)
            .map_err(|_| "the current lockfile cannot be loaded")?,
    };
    let Some(current) = current else {
        return Ok(());
    };
    current
        .save_to_path(&check.workspace_root.join(check.config.wanted_lockfile_name()))
        .map_err(|_| "failed to regenerate the wanted lockfile from the current lockfile")
}

impl<'a> LinkedPackagesContext<'a> {
    fn new(config: &Config, project_manifests: &'a [(PathBuf, &'a PackageManifest)]) -> Self {
        let mut manifests_by_dir = std::collections::HashMap::new();
        let mut workspace_packages: std::collections::HashMap<
            String,
            std::collections::HashMap<String, &'a Path>,
        > = std::collections::HashMap::new();
        for (root_dir, manifest) in project_manifests {
            manifests_by_dir.insert(root_dir.as_path(), *manifest);
            if let (Some(name), Some(version)) = (
                manifest_string_field(manifest, "name"),
                manifest_string_field(manifest, "version"),
            ) {
                workspace_packages.entry(name).or_default().insert(version, root_dir.as_path());
            }
        }
        LinkedPackagesContext {
            link_workspace_packages: config.link_workspace_packages != LinkWorkspacePackages::Off,
            manifests_by_dir,
            workspace_packages,
        }
    }

    /// The version of the package manifest at `dir`, preferring the
    /// already-loaded workspace manifests over a disk read.
    fn linked_version(&self, dir: &Path) -> Option<String> {
        if let Some(manifest) = self.manifests_by_dir.get(dir) {
            return manifest_string_field(manifest, "version");
        }
        pnpm_package_manifest::safe_read_package_json_from_dir(dir)
            .ok()
            .flatten()
            .and_then(|value| value.get("version").and_then(|v| v.as_str()).map(str::to_string))
    }
}

fn current_lockfile_unusable_with_non_empty_wanted(
    check: &OptimisticRepeatInstallCheck<'_>,
) -> Result<bool, &'static str> {
    if check.is_workspace_install || !check.config.lockfile {
        return Ok(false);
    }
    if current_lockfile_file_has_content(&check.config.virtual_store_dir) {
        return Ok(false);
    }
    let Some(wanted) =
        check.lockfile.get().map_err(|_| "the wanted lockfile cannot be read or parsed")?
    else {
        return Ok(false);
    };
    Ok(!wanted.is_empty())
}

fn current_lockfile_file_has_content(virtual_store_dir: &Path) -> bool {
    fs::metadata(virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME))
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

/// Project count + per-project (key, name, version) match between the
/// cached state and today's walk. The key is the project's root dir;
/// `build_workspace_state` and pnpm both use it as the map key, so a
/// renamed / removed / added project trips the check immediately.
fn project_structure_matches(
    state: &WorkspaceState,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    if state.projects.len() != project_manifests.len() {
        return false;
    }
    project_manifests.iter().all(|(root_dir, manifest)| {
        let key = root_dir.to_string_lossy().into_owned();
        let Some(entry) = state.projects.get(&key) else {
            return false;
        };
        entry.name.as_deref() == manifest_string_field(manifest, "name").as_deref()
            && entry.version.as_deref().unwrap_or("0.0.0")
                == manifest_string_field(manifest, "version").as_deref().unwrap_or("0.0.0")
    })
}

fn modules_dirs_present(
    config: &Config,
    node_linker: NodeLinker,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    first_project_missing_modules_dir(config, node_linker, project_manifests).is_none()
}

/// The id (`name` field, falling back to the root dir) of the first
/// project that declares dependencies but has no modules directory, or
/// `None` when every project with dependencies has one.
fn first_project_missing_modules_dir(
    config: &Config,
    node_linker: NodeLinker,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> Option<String> {
    let root_modules_dir_exists = config.modules_dir.exists();

    project_manifests.iter().find_map(|(root_dir, manifest)| {
        if !manifest_has_runtime_deps(manifest) {
            return None;
        }
        // The root importer uses `config.modules_dir`; siblings use
        // their own `<root>/node_modules`. Matches the isolated-linker
        // default — `config.modules_dir` is `<workspace_root>/node_modules`
        // unless the user overrode it explicitly.
        let modules_dir_exists = match node_linker {
            NodeLinker::Hoisted => root_modules_dir_exists,
            NodeLinker::Isolated | NodeLinker::Pnp => {
                if *root_dir == workspace_dir_of(config, root_dir) {
                    root_modules_dir_exists
                } else {
                    root_dir.join("node_modules").exists()
                }
            }
        };

        (!modules_dir_exists).then(|| {
            manifest_string_field(manifest, "name")
                .unwrap_or_else(|| root_dir.to_string_lossy().into_owned())
        })
    })
}

/// Recover the workspace root from `config.modules_dir`. The root
/// importer's `root_dir` equals `config.modules_dir.parent()` because
/// `config.modules_dir` is `<workspace_root>/node_modules`. Used by
/// [`modules_dirs_present`] to tell root from sibling — a brittle
/// shape but it matches how the install path itself derives
/// `config.modules_dir`.
fn workspace_dir_of(config: &Config, fallback: &Path) -> PathBuf {
    config.modules_dir.parent().map_or_else(|| fallback.to_path_buf(), Path::to_path_buf)
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
