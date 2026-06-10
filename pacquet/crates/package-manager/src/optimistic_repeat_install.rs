//! Pre-install fast path: when nothing has changed since the last
//! install, skip the entire pipeline.
//!
//! Port of upstream's `optimisticRepeatInstall` + [`checkDepsStatus`]
//! dispatch. `installDeps` calls `checkDepsStatus` before any of the
//! install setup runs and logs "Already up to date" when nothing has
//! changed. The check keys off `<workspace_root>/node_modules/.pnpm-workspace-state-v1.json`'s
//! `lastValidatedTimestamp` against each project's `package.json`
//! mtime — never touching the lockfile, the verifier cache, or any
//! resolver state.
//!
//! Mirrors:
//! - [`installing/commands/src/installDeps.ts:179-194`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/installing/commands/src/installDeps.ts#L179-L194)
//!   — dispatch.
//! - [`deps/status/src/checkDepsStatus.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts)
//!   — the underlying check.
//!
//! Scope: the mtime-vs-`lastValidatedTimestamp` branch (upstream's
//! `modifiedProjects.length === 0` exit at lines 263-271), the
//! patch-file branch of upstream's
//! [`patchesOrHooksAreModified`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L597-L612)
//! (a configured patch file whose mtime is newer than
//! `lastValidatedTimestamp` invalidates the fast path even when its
//! `patchedDependencies` config entry is unchanged — a content edit the
//! key→path settings comparison can't see), and the modified-manifests
//! content re-check: when a manifest's mtime is newer but its
//! dependency-relevant content still matches the lockfile, upstream's
//! [`assertWantedLockfileUpToDate`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L483-L561)
//! still reports up-to-date (a `touch package.json`, a `scripts` edit, or
//! an `npm pkg set/delete` rewrite must not trigger a full install). The
//! pnpmfile branch of `patchesOrHooksAreModified` and the
//! `isLocalFileDepUpdated` branch of `linkedPackagesAreUpToDate` are NOT
//! ported here. When this function returns `Decision::Skipped` the caller
//! proceeds with the full install path, which still has its own freshness
//! guards (`check_lockfile_freshness`, the no-op short-circuit). Remaining
//! work tracked at pnpm/pnpm#11940 (this issue).
//!
//! [`checkDepsStatus`]: https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts
//!
//! ## Why a separate module
//!
//! Lives in `pacquet-package-manager` rather than a new
//! `pacquet-deps-status` crate because the only call site today is
//! `Install::run`. When pacquet ports `verifyDepsBeforeRun` (the
//! second consumer of `checkDepsStatus` upstream), extract this into
//! its own crate to match pnpm's `@pnpm/deps.status` package.

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use pacquet_catalogs_types::Catalogs;
use pacquet_config::{Config, LinkWorkspacePackages, NodeLinker};
use pacquet_lockfile::{ImporterDepVersion, Lockfile, ProjectSnapshot};
use pacquet_modules_yaml::IncludedDependencies;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_workspace_state::{
    NodeLinker as WorkspaceStateNodeLinker, WorkspaceState, WorkspaceStateSettings,
    load_workspace_state, update_workspace_state,
};

/// Outcome of [`check_optimistic_repeat_install`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The install is fully up to date — emit "Already up to date"
    /// and exit before any of the install setup runs. Mirrors pnpm's
    /// [`upToDate: true`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L270)
    /// outcome.
    UpToDate,
    /// Fall through to the full install path. `reason` is the short
    /// string the upstream `issue` field would carry; surfaced via
    /// `tracing::debug!` for diagnosability without contaminating the
    /// reporter stream.
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
    /// Every importer's `(root_dir, manifest)` pair. For a
    /// single-project install it's just the root manifest; for a
    /// workspace install it's every project the resolver would
    /// otherwise walk. The caller passes this in (rather than this
    /// function rediscovering it) so the same walk seeds the regular
    /// install path on the fall-through.
    pub project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    /// `true` when a `pnpm-workspace.yaml` drives the install — that
    /// selects pnpm's
    /// [`allProjects && workspaceDir` branch](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L187)
    /// which keys the manifest and lockfile comparisons off
    /// `lastValidatedTimestamp`. `false` (no workspace manifest)
    /// selects the
    /// [single-project branch](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L387-L462)
    /// which additionally requires `pnpm-lock.yaml` to exist on disk —
    /// pnpm throws `RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND` otherwise, which
    /// the outer `try` converts into `upToDate: false` — and keys its
    /// comparisons off the lockfile mtimes instead.
    pub is_workspace_install: bool,
    /// The wanted lockfile as loaded by the CLI (`None` when
    /// `pnpm-lock.yaml` is absent or empty). Consulted only by the
    /// modified-manifests content re-check; the pure-mtime fast path
    /// never reads it. When `None` and `<virtual_store_dir>/lock.yaml`
    /// exists, the current lockfile stands in as the wanted one — it
    /// records exactly what the previous install materialized — and
    /// `pnpm-lock.yaml` is regenerated from it before the check
    /// reports up-to-date.
    pub lockfile: Option<&'a Lockfile>,
    /// Workspace catalogs, for resolving `catalog:` values inside
    /// `pnpm.overrides` before the lockfile settings comparison.
    pub catalogs: &'a Catalogs,
}

/// Run the workspace-state freshness fast path. Returns
/// [`Decision::UpToDate`] when the install can short-circuit.
///
/// Always returns `Decision::Skipped` when
/// `config.optimistic_repeat_install` is `false`.
pub fn check_optimistic_repeat_install(check: &OptimisticRepeatInstallCheck<'_>) -> Decision {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        node_linker,
        included,
        project_manifests,
        is_workspace_install,
        ..
    } = check;
    if !config.optimistic_repeat_install {
        return Decision::Skipped { reason: "optimistic_repeat_install disabled" };
    }

    // No workspace state means no previous install has completed
    // (or the file was deleted) — there's no `lastValidatedTimestamp`
    // to compare against. Mirrors upstream's
    // <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L80-L86>
    // first-return guard.
    let Ok(Some(state)) = load_workspace_state(workspace_root) else {
        return Decision::Skipped { reason: "no workspace state on disk" };
    };

    if !settings_match(&state, config, node_linker, included) {
        return Decision::Skipped { reason: "settings drift" };
    }

    if !project_structure_matches(&state, project_manifests) {
        return Decision::Skipped { reason: "workspace project list changed" };
    }

    // The "modules dir exists when the project has deps" gate from
    // upstream's
    // <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L237-L249>:
    // a project with `dependencies`/`devDependencies` but no
    // `node_modules` cannot be up to date. Pnpm reads `modulesDir`
    // off the per-project config; pacquet doesn't track per-importer
    // overrides yet, so check the install-time `config.modules_dir`
    // for the root + `<project_root>/node_modules` for siblings,
    // matching pnpm's `isolated`-linker default.
    if !modules_dirs_present(config, project_manifests) {
        return Decision::Skipped {
            reason: "project has dependencies but no node_modules directory",
        };
    }

    // Single-project installs require a lockfile to even attempt the
    // fast path. Upstream's single-project branch at
    // <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L396-L401>
    // throws `RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND` when
    // `wantedLockfileStats` is absent, which the outer `try`
    // converts into `upToDate: false`. Pacquet additionally accepts
    // the *current* lockfile (`<virtual_store_dir>/lock.yaml`) as a
    // stand-in when `pnpm-lock.yaml` is missing: it records exactly
    // what the previous install materialized, so the content checks
    // can run against it and `pnpm-lock.yaml` is regenerated from it
    // on success — the same substitution the full install path makes
    // when it synthesizes the wanted lockfile from the current one.
    // Workspace installs skip this gate — pnpm's workspace branch
    // returns `upToDate: true` purely off the manifest-mtime check
    // (its only lockfile probe, `findConflictedLockfileDir`, silently
    // `continue`s on ENOENT at
    // <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L593-L596>).
    if !is_workspace_install
        && !workspace_root.join(Lockfile::FILE_NAME).exists()
        && !config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME).exists()
    {
        return Decision::Skipped { reason: "wanted lockfile missing" };
    }

    // A patch file edited in place keeps the same `patchedDependencies`
    // key→path entry (so `settings_match` can't see the change) but
    // changes the patched output and the patch hash. Upstream catches
    // this in `patchesOrHooksAreModified` before the manifest-modified
    // exit; mirror that ordering so the patch reason wins when both a
    // patch and a manifest are newer than the last validation.
    if patches_modified_since(workspace_root, config, state.last_validated_timestamp) {
        return Decision::Skipped { reason: "a patch file is newer than the last validation" };
    }

    // The fast-path conclusion. Upstream walks every manifest and
    // returns `upToDate: true` when none have an mtime newer than
    // `workspaceState.lastValidatedTimestamp`. The walk has to
    // succeed (read errors mean we can't *prove* freshness, so fall
    // through).
    let Some(manifest_stats) = stat_manifests(project_manifests) else {
        return Decision::Skipped { reason: "failed to stat a project manifest" };
    };
    let modified: Vec<&ManifestStat<'_>> = manifest_stats
        .iter()
        .filter(|stat| stat.mtime_ms > state.last_validated_timestamp)
        .collect();
    if modified.is_empty() {
        return match regenerate_wanted_lockfile_if_missing(check, None) {
            Ok(()) => Decision::UpToDate,
            Err(reason) => Decision::Skipped { reason },
        };
    }

    // A newer mtime alone doesn't invalidate: upstream's
    // modified-manifests branch re-checks the *content* against the
    // wanted lockfile (`assertWantedLockfileUpToDate`) so a rewrite
    // that left the dependency fields intact — `touch`, a `scripts`
    // edit, `npm pkg set/delete` — still reports up to date.
    match modified_manifests_match_lockfile(check, &state, &modified) {
        Ok(loaded_current) => {
            if let Err(reason) = regenerate_wanted_lockfile_if_missing(check, loaded_current) {
                return Decision::Skipped { reason };
            }
            // "update lastValidatedTimestamp to prevent pointless
            // repeat" — upstream's workspace branch rewrites the
            // state after the content checks pass at
            // <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L349-L357>.
            // The single-project branch keys its comparisons off the
            // lockfile mtimes instead and leaves the state alone. A
            // failed write only costs the next run a repeat of the
            // content check, so it degrades rather than fails.
            if is_workspace_install {
                let new_state = crate::install::build_workspace_state(
                    config,
                    node_linker,
                    included,
                    project_manifests,
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
    if check.lockfile.is_some() || !check.config.lockfile {
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
        .save_to_path(&check.workspace_root.join(Lockfile::FILE_NAME))
        .map_err(|_| "failed to regenerate pnpm-lock.yaml from the current lockfile")
}

/// One project manifest's stat outcome, paired with the inputs the
/// content re-check needs.
struct ManifestStat<'a> {
    root_dir: &'a Path,
    manifest: &'a PackageManifest,
    mtime_ms: i64,
}

/// Port of upstream's modified-manifests branch: the lockfile-equality
/// assertion ([`assertLockfilesEqual`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/assertLockfilesEqual.ts))
/// plus [`assertWantedLockfileUpToDate`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L483-L561)
/// (settings drift, per-importer specifier match, linked-package
/// freshness) for every project whose manifest is newer than the last
/// validation. `Err` carries the `Decision::Skipped` reason; upstream
/// converts the equivalent throws into `upToDate: false`.
///
/// When `pnpm-lock.yaml` is absent, the current lockfile stands in as
/// the wanted one (see the lockfile gate in
/// [`check_optimistic_repeat_install`]); `Ok(Some(_))` then carries the
/// loaded current lockfile so the caller can regenerate
/// `pnpm-lock.yaml` from it without a second read.
fn modified_manifests_match_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    modified: &[&ManifestStat<'_>],
) -> Result<Option<Lockfile>, &'static str> {
    let &OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        project_manifests,
        is_workspace_install,
        lockfile,
        catalogs,
        ..
    } = check;
    let mut loaded_current: Option<Lockfile> = None;
    let mut wanted_is_current = false;
    let (wanted, wanted_mtime_ms): (&Lockfile, i64) = if let Some(wanted) = lockfile {
        let Some(mtime) = mtime_ms(&workspace_root.join(Lockfile::FILE_NAME)) else {
            return Err(
                "a manifest is newer than the last validation and pnpm-lock.yaml cannot be stat'd",
            );
        };
        (wanted, mtime)
    } else {
        let current_path = config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
        let Some(mtime) = mtime_ms(&current_path) else {
            return Err("a manifest is newer than the last validation and no lockfile is loaded");
        };
        let current = Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir)
            .map_err(|_| "the current lockfile cannot be loaded")?
            .ok_or("a manifest is newer than the last validation and no lockfile is loaded")?;
        wanted_is_current = true;
        (&*loaded_current.insert(current), mtime)
    };

    // Decide which modified projects need the full content check, and
    // whether the wanted lockfile must be compared against the current
    // one (`<virtual_store_dir>/lock.yaml`).
    let to_check: &[&ManifestStat<'_>] = if wanted_is_current {
        // The wanted lockfile IS the current one — there's no second
        // lockfile to assert equality against, and the mtime
        // short-circuits below compare the two lockfile files, so they
        // don't apply. Every modified project gets the content check.
        modified
    } else if is_workspace_install {
        // Workspace branch: a wanted lockfile newer than the last
        // validation must equal what the previous install materialized.
        // Mirrors <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L283-L289>.
        if wanted_mtime_ms > state.last_validated_timestamp {
            assert_wanted_lockfile_equals_current(wanted, config)?;
        }
        modified
    } else {
        // Single-project branch keys off the lockfile mtimes instead of
        // `lastValidatedTimestamp`. Mirrors
        // <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L407-L462>.
        let current_mtime_ms =
            mtime_ms(&config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME));
        if let Some(current_mtime_ms) = current_mtime_ms
            && wanted_mtime_ms > current_mtime_ms
        {
            assert_wanted_lockfile_equals_current(wanted, config)?;
        }
        let root = modified.first().expect("modified-manifests branch requires a modified project");
        if root.mtime_ms > wanted_mtime_ms {
            modified
        } else if current_mtime_ms.is_some() {
            // "The manifest file is not newer than the lockfile.
            // Exiting check."
            &[]
        } else if wanted.packages.as_ref().is_some_and(|packages| !packages.is_empty()) {
            // RUN_CHECK_DEPS_NO_DEPS: the lockfile requires
            // dependencies but nothing was ever installed.
            return Err("the lockfile requires dependencies but none were installed");
        } else {
            &[]
        }
    };

    if to_check.is_empty() {
        return Ok(loaded_current);
    }

    let parsed_overrides = crate::install::parse_config_overrides(config, catalogs)
        .map_err(|_| "pnpm.overrides cannot be parsed")?;
    if let Err(error) =
        crate::install::check_lockfile_settings_drift(wanted, config, parsed_overrides.as_deref())
    {
        tracing::debug!(target: "pacquet::install", %error, "repeat-install content check: lockfile settings drift");
        return Err("a lockfile setting drifted from the current configuration");
    }

    let linked_ctx = LinkedPackagesContext::new(config, project_manifests);
    for project in to_check {
        let importer_id =
            pacquet_workspace::importer_id_from_root_dir(workspace_root, project.root_dir);
        if let Err(error) = crate::install::check_importer_satisfies(
            wanted,
            project.manifest,
            &importer_id,
            config,
            parsed_overrides.as_deref(),
        ) {
            tracing::debug!(target: "pacquet::install", %error, importer_id, "repeat-install content check: manifest no longer satisfied");
            return Err("a modified manifest is no longer satisfied by the lockfile");
        }
        let Some(importer) = wanted.importers.get(&importer_id) else {
            return Err("a modified project has no importer entry in the lockfile");
        };
        if !linked_packages_are_up_to_date(
            &linked_ctx,
            project.root_dir,
            project.manifest,
            importer,
        ) {
            return Err("a linked package is out of date");
        }
    }
    Ok(loaded_current)
}

/// Port of upstream's
/// [`assertLockfilesEqual`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/assertLockfilesEqual.ts):
/// with no current lockfile every importer of the wanted one must be
/// dependency-free (`RUN_CHECK_DEPS_NO_DEPS`); otherwise the two parsed
/// lockfiles must be equal (`RUN_CHECK_DEPS_OUTDATED_DEPS`).
fn assert_wanted_lockfile_equals_current(
    wanted: &Lockfile,
    config: &Config,
) -> Result<(), &'static str> {
    let current = Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir)
        .map_err(|_| "the current lockfile cannot be loaded")?;
    match current {
        None => {
            let any_deps = wanted.importers.values().any(|snapshot| {
                snapshot
                    .dependencies_by_groups([
                        DependencyGroup::Prod,
                        DependencyGroup::Dev,
                        DependencyGroup::Optional,
                    ])
                    .next()
                    .is_some()
            });
            if any_deps {
                Err("the lockfile requires dependencies but none were installed")
            } else {
                Ok(())
            }
        }
        Some(current) => {
            if &current == wanted {
                Ok(())
            } else {
                Err("the installed dependencies are not up to date with the lockfile")
            }
        }
    }
}

/// Shared lookups for [`linked_packages_are_up_to_date`], built once
/// per content check. Mirrors the `bind(null, {...})` context upstream
/// creates in
/// [`checkDepsStatus`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L317-L329).
struct LinkedPackagesContext<'a> {
    link_workspace_packages: bool,
    manifests_by_dir: std::collections::HashMap<&'a Path, &'a PackageManifest>,
    /// `name → version → root_dir` over the workspace's projects —
    /// the same index upstream's `workspacePackages` map carries.
    workspace_packages:
        std::collections::HashMap<String, std::collections::HashMap<String, &'a Path>>,
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
    /// already-loaded workspace manifests over a disk read. Mirrors
    /// upstream's `manifestsByDir[linkedDir] ?? safeReadPackageJsonFromDir`.
    fn linked_version(&self, dir: &Path) -> Option<String> {
        if let Some(manifest) = self.manifests_by_dir.get(dir) {
            return manifest_string_field(manifest, "version");
        }
        pacquet_package_manifest::safe_read_package_json_from_dir(dir)
            .ok()
            .flatten()
            .and_then(|value| value.get("version").and_then(|v| v.as_str()).map(str::to_string))
    }
}

/// Port of upstream's
/// [`linkedPackagesAreUpToDate`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/lockfile/verification/src/linkedPackagesAreUpToDate.ts):
/// every importer dependency that resolved to a workspace link must
/// still link under today's manifest spec, and every one that resolved
/// to the registry must not have become linkable. The
/// `isLocalFileDepUpdated` branch (a `file:` directory specifier) is
/// not ported — those entries conservatively report "not up to date" so
/// the full install path re-evaluates them.
fn linked_packages_are_up_to_date(
    ctx: &LinkedPackagesContext<'_>,
    project_dir: &Path,
    manifest: &PackageManifest,
    snapshot: &ProjectSnapshot,
) -> bool {
    const GROUPS: [(DependencyGroup, &str); 3] = [
        (DependencyGroup::Optional, "optionalDependencies"),
        (DependencyGroup::Prod, "dependencies"),
        (DependencyGroup::Dev, "devDependencies"),
    ];
    for (group, manifest_field) in GROUPS {
        let Some(lockfile_deps) = snapshot.get_map_by_group(group) else {
            continue;
        };
        let Some(manifest_deps) =
            manifest.value().get(manifest_field).and_then(|value| value.as_object())
        else {
            continue;
        };
        for (dep_name, dep) in lockfile_deps {
            let dep_name = dep_name.to_string();
            let Some(current_spec) = manifest_deps.get(&dep_name).and_then(|v| v.as_str()) else {
                continue;
            };
            if ref_is_local_directory(&dep.specifier) {
                // A `file:` specifier that resolved to `link:` (e.g. an
                // injected self-reference) is a local link with no
                // `packages:` entry — up to date by construction.
                if matches!(dep.version, ImporterDepVersion::Link(_)) {
                    continue;
                }
                return false;
            }
            let link_target = dep.version.as_link_target();
            let is_linked = link_target.is_some();
            if is_linked
                && (current_spec.starts_with("link:")
                    || current_spec.starts_with("file:")
                    || current_spec.starts_with("workspace:."))
            {
                continue;
            }
            // A linked dependency whose spec is a distribution tag is
            // considered up to date to skip full resolution
            // (<https://github.com/pnpm/pnpm/issues/6592>).
            if is_linked && spec_is_distribution_tag(current_spec) {
                continue;
            }
            let linked_dir: Option<std::borrow::Cow<'_, Path>> = match link_target {
                Some(target) => Some(std::borrow::Cow::Owned(project_dir.join(target))),
                None => dep
                    .version
                    .as_regular()
                    .map(std::string::ToString::to_string)
                    .and_then(|version| ctx.workspace_packages.get(&dep_name)?.get(&version))
                    .map(|dir| std::borrow::Cow::Borrowed(*dir)),
            };
            let Some(linked_dir) = linked_dir else {
                continue;
            };
            if !ctx.link_workspace_packages && !current_spec.starts_with("workspace:") {
                // A linkable dir exists, but nothing requests linking it.
                continue;
            }
            let available_range = version_range_of_spec(current_spec);
            let local_package_satisfies_range = matches!(available_range, "*" | "^" | "~")
                || ctx
                    .linked_version(&linked_dir)
                    .is_some_and(|version| semver_satisfies_loosely(&version, available_range));
            if is_linked != local_package_satisfies_range {
                return false;
            }
        }
    }
    true
}

/// Port of upstream's
/// [`refIsLocalDirectory`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/lockfile/utils/src/refIsLocalTarball.ts):
/// a `file:` specifier that is not a tarball.
fn ref_is_local_directory(specifier: &str) -> bool {
    specifier.starts_with("file:")
        && !(specifier.ends_with(".tgz")
            || specifier.ends_with(".tar.gz")
            || specifier.ends_with(".tar"))
}

/// Whether a bare specifier is an npm distribution tag (`latest`,
/// `beta`, ...). Approximates upstream's
/// `getVersionSelectorType(spec)?.type === 'tag'` — anything that
/// doesn't parse as a semver range and contains only characters a tag
/// name may carry. Protocol-ish specs (`workspace:^1.0.0`,
/// `npm:foo@1`) contain `:`/`@`/`/` and therefore never match, same as
/// `version-selector-type` rejecting them.
fn spec_is_distribution_tag(spec: &str) -> bool {
    !spec.is_empty()
        && spec.parse::<node_semver::Range>().is_err()
        && spec.chars().all(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_' | '.'))
}

/// Port of upstream's `getVersionRange`: strips the `workspace:` /
/// `npm:` envelope so the remainder can be compared as a semver range.
fn version_range_of_spec(spec: &str) -> &str {
    if let Some(rest) = spec.strip_prefix("workspace:") {
        return rest;
    }
    if let Some(rest) = spec.strip_prefix("npm:") {
        // `npm:<alias>@<range>` — the `@` search starts at index 1 so a
        // leading scope `@` isn't mistaken for the separator.
        return match rest.get(1..).and_then(|tail| tail.find('@')) {
            Some(at) => {
                let range = &rest[at + 2..];
                if range.is_empty() { "*" } else { range }
            }
            None => "*",
        };
    }
    spec
}

/// `semver.satisfies(version, range, { loose: true })` — a version or
/// range that doesn't parse fails the match.
fn semver_satisfies_loosely(version: &str, range: &str) -> bool {
    let Ok(version) = version.parse::<node_semver::Version>() else { return false };
    let Ok(range) = range.parse::<node_semver::Range>() else { return false };
    range.satisfies(&version)
}

/// Millisecond mtime of `path`, `None` when it can't be stat'd.
/// Converts wall-clock to ms-since-epoch the same way
/// `pacquet_workspace_state::now_millis` does on the write side, so
/// comparisons against `lastValidatedTimestamp` are apples-to-apples.
fn mtime_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).and_then(|metadata| metadata.modified()).ok()?;
    let elapsed = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

/// Compare today's settings against what the previous install
/// recorded.
///
/// Only the fields pacquet populates via [`current_settings`]
/// participate in the comparison; the rest are listed at the end of
/// this function with the reason each is safe to skip.
///
/// pnpm's [`checkDepsStatus`](https://github.com/pnpm/pnpm/blob/20f9362161/deps/status/src/checkDepsStatus.ts#L138)
/// iterates the full `WORKSPACE_STATE_SETTING_KEYS` list, reading a key
/// absent from the recorded state as `undefined`. So the reverse
/// scenario (pacquet wrote the state, pnpm reads it next) stays on the
/// fast path only for keys whose pnpm-resolved value is also
/// `undefined`. Every key pnpm resolves to a concrete default —
/// `excludeLinksFromLockfile` (`false`), `minimumReleaseAge` (`1440`),
/// `minimumReleaseAgeIgnoreMissingTime` (`true`) — must therefore be
/// written by [`current_settings`] and compared here, or pnpm would
/// report drift and re-run a (no-op) install on every command after a
/// pacquet install. `enableGlobalVirtualStore` is `undefined` by
/// default (concrete only under `--global`/CI), so pacquet's omit-when-
/// off encoding already matches. The `allowBuilds` coercion mirrors
/// pnpm's [`opts.allowBuilds ?? {}`](https://github.com/pnpm/pnpm/blob/20f9362161/deps/status/src/checkDepsStatus.ts#L143)
/// on the read side and pnpm's tolerance of an absent `allowBuilds` key
/// in the recorded state on the write side.
fn settings_match(
    state: &WorkspaceState,
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
) -> bool {
    let current = current_settings(config, node_linker, included);
    let recorded = &state.settings;
    let live = &current;
    allow_builds_match(recorded.allow_builds.as_ref(), live.allow_builds.as_ref())
        && recorded.auto_install_peers == live.auto_install_peers
        && recorded.dedupe_direct_deps == live.dedupe_direct_deps
        && recorded.dedupe_injected_deps == live.dedupe_injected_deps
        && recorded.dedupe_peer_dependents == live.dedupe_peer_dependents
        && recorded.dedupe_peers == live.dedupe_peers
        && recorded.dev == live.dev
        && enable_global_virtual_store_match(
            recorded.enable_global_virtual_store,
            live.enable_global_virtual_store,
        )
        && recorded.exclude_links_from_lockfile == live.exclude_links_from_lockfile
        && recorded.hoist_pattern == live.hoist_pattern
        && recorded.hoist_workspace_packages == live.hoist_workspace_packages
        && recorded.ignored_optional_dependencies == live.ignored_optional_dependencies
        && recorded.inject_workspace_packages == live.inject_workspace_packages
        && recorded.link_workspace_packages == live.link_workspace_packages
        && recorded.minimum_release_age == live.minimum_release_age
        && recorded.minimum_release_age_ignore_missing_time
            == live.minimum_release_age_ignore_missing_time
        && recorded.node_linker == live.node_linker
        && recorded.optional == live.optional
        && recorded.overrides == live.overrides
        && package_extensions_match(
            recorded.package_extensions.as_ref(),
            live.package_extensions.as_ref(),
        )
        && recorded.patched_dependencies == live.patched_dependencies
        && recorded.peers_suffix_max_length == live.peers_suffix_max_length
        && recorded.prefer_workspace_packages == live.prefer_workspace_packages
        && recorded.production == live.production
        && recorded.public_hoist_pattern == live.public_hoist_pattern
    // Deliberately *not* compared. pnpm leaves the first group
    // `undefined` by default, so omitting them here still matches pnpm's
    // all-key freshness check (`undefined == undefined`):
    //   catalogs                    (pnpm always ignores; see
    //                                ignoredSettings.add('catalogs'))
    //   minimumReleaseAgeStrict     (pnpm sets it only when the user
    //                                explicitly sets minimumReleaseAge)
    //   minimumReleaseAgeExclude
    //   trustPolicy*                (all `undefined` until configured)
    //   workspacePackagePatterns    (concrete for a multi-package
    //                                workspace, but lives in the
    //                                workspace manifest, not `Config`;
    //                                threading it into `current_settings`
    //                                is a separate follow-up. pacquet
    //                                detects project-set changes via
    //                                `project_structure_matches`).
}

/// `enableGlobalVirtualStore` has no `?? default` coercion on pnpm's
/// read side, but its `undefined` default and an explicit `false` both
/// mean "global virtual store off". pnpm omits the key for the former
/// and records `false` only when CI forces it; pacquet omits both.
/// Normalize the absent and `false` forms before comparing so a
/// pnpm-written file (omitted or `false`) matches a pacquet install
/// with the store off, while a real `true`/`false` flip still trips.
fn enable_global_virtual_store_match(
    state_value: Option<bool>,
    current_value: Option<bool>,
) -> bool {
    state_value.unwrap_or(false) == current_value.unwrap_or(false)
}

/// Pnpm writes `Some({})` for an empty `allowBuilds`; pacquet writes
/// `None` for the same effective value. Treat them as equivalent so
/// cross-package-manager state files don't trip the comparison.
fn allow_builds_match(
    state_value: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
    current_value: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
) -> bool {
    match (state_value, current_value) {
        (None, None) => true,
        (Some(map), None) | (None, Some(map)) => map.is_empty(),
        (Some(state_map), Some(current_map)) => state_map == current_map,
    }
}

/// `packageExtensions` are compared as opaque `serde_json::Value`
/// trees so the workspace-state file written by either implementation
/// round-trips through the other. Empty maps are equivalent to absent
/// — pacquet's [`pacquet_config::WorkspaceSettings::apply_to`] already collapses
/// `packageExtensions: {}` to `None`, but pnpm may write `Some({})`
/// directly, and the workspace-state file is shared across the two.
fn package_extensions_match(
    state_value: Option<&serde_json::Value>,
    current_value: Option<&serde_json::Value>,
) -> bool {
    fn is_empty(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => map.is_empty(),
            serde_json::Value::Null => true,
            _ => false,
        }
    }
    match (state_value, current_value) {
        (None, None) => true,
        (Some(value), None) | (None, Some(value)) => is_empty(value),
        (Some(state_value), Some(current_value)) => state_value == current_value,
    }
}

/// Build the [`WorkspaceStateSettings`] that today's install would
/// write. Shared with `install::build_workspace_state` so the
/// freshness check sees the same byte shape the writer produced —
/// when one side grows a field, the other automatically does too.
pub(crate) fn current_settings(
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
) -> WorkspaceStateSettings {
    let allow_builds = (!config.allow_builds.is_empty()).then(|| {
        config.allow_builds.iter().map(|(k, v)| (k.clone(), serde_json::Value::Bool(*v))).collect()
    });
    WorkspaceStateSettings {
        allow_builds,
        auto_install_peers: Some(config.auto_install_peers),
        dedupe_direct_deps: Some(config.dedupe_direct_deps),
        dedupe_injected_deps: Some(config.dedupe_injected_deps),
        dedupe_peer_dependents: Some(config.dedupe_peer_dependents),
        dedupe_peers: Some(config.dedupe_peers),
        dev: Some(included.dev_dependencies),
        // Mirror pnpm's writer, which omits the key for its `undefined`
        // default and records a concrete value only when forced. pacquet
        // has no `--global` flow, so the only "on" value it ever writes
        // is `true`; an off store maps back to the omitted `None`.
        enable_global_virtual_store: config.enable_global_virtual_store.then_some(true),
        exclude_links_from_lockfile: Some(config.exclude_links_from_lockfile),
        hoist_pattern: config.hoist_pattern.clone(),
        hoist_workspace_packages: Some(config.hoist_workspace_packages),
        ignored_optional_dependencies: config.ignored_optional_dependencies.clone(),
        inject_workspace_packages: Some(config.inject_workspace_packages),
        link_workspace_packages: Some(link_workspace_packages_to_json(
            config.link_workspace_packages,
        )),
        minimum_release_age: config.minimum_release_age,
        minimum_release_age_ignore_missing_time: Some(
            config.minimum_release_age_ignore_missing_time,
        ),
        node_linker: Some(map_node_linker(node_linker)),
        optional: Some(included.optional_dependencies),
        overrides: config
            .overrides
            .as_ref()
            .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        package_extensions: config
            .package_extensions
            .as_ref()
            .and_then(|map| serde_json::to_value(map).ok()),
        patched_dependencies: config.patched_dependencies.clone(),
        peers_suffix_max_length: Some(
            u32::try_from(config.peers_suffix_max_length).unwrap_or(u32::MAX),
        ),
        prefer_workspace_packages: Some(config.prefer_workspace_packages),
        production: Some(included.dependencies),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        ..Default::default()
    }
}

fn link_workspace_packages_to_json(value: LinkWorkspacePackages) -> serde_json::Value {
    match value {
        LinkWorkspacePackages::Off => serde_json::Value::Bool(false),
        LinkWorkspacePackages::DirectOnly => serde_json::Value::Bool(true),
        LinkWorkspacePackages::Deep => serde_json::Value::String("deep".to_string()),
    }
}

fn map_node_linker(linker: NodeLinker) -> WorkspaceStateNodeLinker {
    match linker {
        NodeLinker::Isolated => WorkspaceStateNodeLinker::Isolated,
        NodeLinker::Hoisted => WorkspaceStateNodeLinker::Hoisted,
        NodeLinker::Pnp => WorkspaceStateNodeLinker::Pnp,
    }
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
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> bool {
    project_manifests.iter().all(|(root_dir, manifest)| {
        if !manifest_has_runtime_deps(manifest) {
            return true;
        }
        // The root importer uses `config.modules_dir`; siblings use
        // their own `<root>/node_modules`. Matches the isolated-linker
        // default — `config.modules_dir` is `<workspace_root>/node_modules`
        // unless the user overrode it explicitly.
        let modules_dir = if *root_dir == workspace_dir_of(config, root_dir) {
            config.modules_dir.clone()
        } else {
            root_dir.join("node_modules")
        };
        modules_dir.exists()
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
/// validation. Mirrors the patch branch of upstream's
/// [`patchesOrHooksAreModified`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L604-L613):
/// `allPatchStats.some(patch => patch && patch.mtime > lastValidatedTimestamp)`.
/// A patch that can't be stat'd is treated as not-modified — pnpm's
/// `safeStat` returns null and the `patch &&` guard drops it, leaving a
/// genuinely missing patch to surface on the full install path. Patch
/// paths are resolved against `workspace_root` (the `pnpm-workspace.yaml`
/// dir, where `patchedDependencies` is declared), matching how
/// [`Config::patched_dependency_hashes`] resolves them.
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
        let Ok(modified) = fs::metadata(&path).and_then(|metadata| metadata.modified()) else {
            return false;
        };
        let Ok(elapsed) = modified.duration_since(SystemTime::UNIX_EPOCH) else {
            return false;
        };
        let modified_ms = i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX);
        modified_ms > cutoff_ms
    })
}

/// Stat every project's `package.json`. `None` on any stat failure —
/// "can't prove freshness, fall through" — matching pnpm's
/// [`statManifestFile`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/statManifestFile.ts)
/// behavior on missing files (it throws, which `checkDepsStatus`
/// catches via the outer `try`).
fn stat_manifests<'a>(
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
) -> Option<Vec<ManifestStat<'a>>> {
    project_manifests
        .iter()
        .map(|(root_dir, manifest)| {
            mtime_ms(manifest.path()).map(|mtime_ms| ManifestStat {
                root_dir: root_dir.as_path(),
                manifest,
                mtime_ms,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
