use crate::{BuildVerifiersError, InstallFrozenLockfileError, InstallWithFreshLockfileError};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_catalogs_config::InvalidCatalogsConfigurationError;
use pnpm_catalogs_resolver::CatalogResolutionError;
use pnpm_cmd_shim::LinkBinsError;
use pnpm_executor::LifecycleScriptError;
use pnpm_lockfile::{LoadLockfileError, SaveLockfileError, StalenessReason};
use pnpm_lockfile_verification::VerifyError;
use pnpm_modules_yaml::{ReadModulesError, WriteModulesError};
use pnpm_workspace_state::UpdateWorkspaceStateError;
use std::path::PathBuf;

pub(super) fn map_frozen_lockfile_error(error: InstallFrozenLockfileError) -> InstallError {
    match error {
        InstallFrozenLockfileError::LockfileVerification(verify_error) => {
            InstallError::LockfileVerification(verify_error)
        }
        other => InstallError::FrozenLockfile(other),
    }
}
pub(super) fn map_fresh_lockfile_error(error: InstallWithFreshLockfileError) -> InstallError {
    match error {
        InstallWithFreshLockfileError::LockfileVerification(verify_error) => {
            InstallError::LockfileVerification(verify_error)
        }
        other => InstallError::WithFreshLockfile(other),
    }
}
/// Error type of [`Install`](crate::Install).
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallError {
    /// A path named by the `pnpmfile` setting is not on disk. pnpm reports the
    /// same code and message from `requireHooks`.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_NOT_FOUND))]
    MissingPnpmfile(#[error(not(source))] pnpm_hooks::finder::MissingPnpmfileError),
    #[display(
        "Headless installation requires a pnpm-lock.yaml file, but none was found. Run `pnpm install` without --frozen-lockfile to create one."
    )]
    #[diagnostic(code(ERR_PNPM_NO_LOCKFILE))]
    NoLockfile,

    /// A `packageExtensions` selector the freshness gates could not parse.
    /// The resolver reports the same error; this reaches it first because
    /// the gates apply the extensions before deciding whether to resolve.
    #[diagnostic(transparent)]
    InvalidPackageExtensionSelector(
        #[error(source)] crate::package_extender::InvalidPackageExtensionSelector,
    ),

    // The three `*_DIFF` errors below mirror pnpm's `validateModules`:
    // a non-plain-install mutation refuses to touch a modules directory
    // whose persisted layout settings disagree with the current config.
    #[display(
        r#"This modules directory was created using a different hoist-pattern value. Run "pnpm install" to recreate the modules directory."#
    )]
    #[diagnostic(code(ERR_PNPM_HOIST_PATTERN_DIFF))]
    HoistPatternDiff,

    #[display(
        r#"This modules directory was created using a different public-hoist-pattern value. Run "pnpm install" to recreate the modules directory."#
    )]
    #[diagnostic(code(ERR_PNPM_PUBLIC_HOIST_PATTERN_DIFF))]
    PublicHoistPatternDiff,

    #[display(
        r#"This modules directory was created using a different virtual-store-dir-max-length value. Run "pnpm install" to recreate the modules directory."#
    )]
    #[diagnostic(code(ERR_PNPM_VIRTUAL_STORE_DIR_MAX_LENGTH_DIFF))]
    VirtualStoreDirMaxLengthDiff,

    #[diagnostic(transparent)]
    WithFreshLockfile(#[error(source)] InstallWithFreshLockfileError),

    #[diagnostic(transparent)]
    LinkManifestLinkDeps(#[error(source)] crate::LinkManifestLinkDepsError),

    /// pnpm's `ERR_PNPM_IGNORED_BUILDS`: with `strictDepBuilds` on (the
    /// default), an install that blocked any dependency build script
    /// fails so the user explicitly approves the builds. The package
    /// list is the sorted set of `name@version` keys whose scripts were
    /// ignored; the `help` hint matches pnpm's.
    #[display("Ignored build scripts: {}", package_names.join(", "))]
    #[diagnostic(
        code(ERR_PNPM_IGNORED_BUILDS),
        help(
            r#"Run "pnpm approve-builds" to pick which dependencies should be allowed to run scripts."#
        )
    )]
    IgnoredBuilds {
        #[error(not(source))]
        package_names: Vec<String>,
    },

    /// pnpm's `ERR_PNPM_PEER_DEP_ISSUES`: with `strictPeerDependencies`
    /// on, an install whose resolution left unmet peers behind fails
    /// once the artifacts are written, the same way `IgnoredBuilds`
    /// does — the tree is installed, and the run reports the verdict on
    /// it. The listing and its hints have already gone out through the
    /// reporter by the time this is returned.
    #[display("Unmet peer dependencies")]
    #[diagnostic(code(ERR_PNPM_PEER_DEP_ISSUES))]
    PeerDependencyIssues,

    /// A custom resolver hook failed (loading the pnpmfile's resolvers
    /// or running `shouldRefreshResolution`) while deciding whether the
    /// frozen-path optimization may run. A throwing hook aborts the
    /// install.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    CustomResolverForceResolve(#[error(not(source))] pnpm_hooks::HookError),

    /// The pnpmfile's `readPackage` hook threw while transforming a
    /// workspace project's own manifest.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    ReadPackageHook(#[error(not(source))] pnpm_hooks::HookError),

    #[diagnostic(transparent)]
    FrozenLockfile(#[error(source)] InstallFrozenLockfileError),

    /// A workspace project's own lifecycle script
    /// (`pnpm:devPreinstall`, or
    /// preinstall/install/postinstall/preprepare/prepare/postprepare)
    /// exited non-zero. Unlike a dependency build failure — which
    /// `BuildModules` can swallow for optional deps — a project script
    /// failure always fails the install, matching pnpm.
    #[diagnostic(transparent)]
    ProjectLifecycleScript(#[error(source)] LifecycleScriptError),

    #[diagnostic(transparent)]
    ProjectBinLink(#[error(source)] LinkBinsError),

    #[display("Failed to create the workspace lifecycle scheduler: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_LIFECYCLE_THREAD_POOL))]
    ProjectLifecycleThreadPool(#[error(source)] std::io::Error),

    #[display("Unable to determine lifecycle order for workspace projects: {projects}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_LIFECYCLE_ORDER))]
    ProjectLifecycleOrder { projects: String },

    #[diagnostic(transparent)]
    WriteModules(#[error(source)] WriteModulesError),

    /// A filtered install rewrites `.modules.yaml` from the selected
    /// projects' state merged over the previous file's. Without the
    /// previous contents the rewrite would drop every unselected
    /// project's `pendingBuilds` / `ignoredBuilds` / `injectedDeps`, so an
    /// unreadable file fails the install instead of silently pruning it.
    #[diagnostic(transparent)]
    ReadModules(#[error(source)] ReadModulesError),

    /// Surfaces a `pnpm-lock.yaml` read or parse failure from the
    /// deferred load that runs once the repeat-install fast path has
    /// passed on the install (see [`MaybeLazyLockfile`](pnpm_lockfile::MaybeLazyLockfile)).
    #[diagnostic(transparent)]
    LoadWantedLockfile(#[error(source)] LoadLockfileError),

    /// Surfaces a failure to persist the current lockfile so the next
    /// install can diff against it. A best-effort warn would let
    /// silent disk-full or permission issues compound across installs;
    /// fail the install instead.
    #[diagnostic(transparent)]
    SaveCurrentLockfile(#[error(source)] SaveLockfileError),

    /// Surfaces a failure to persist `pnpm-lock.yaml` after the
    /// `cache+node_modules` shortcut regenerated it from the
    /// materialized snapshot at `<virtual_store_dir>/lock.yaml`.
    #[diagnostic(transparent)]
    SaveWantedLockfile(#[error(source)] SaveLockfileError),

    /// Surfaces a failure to delete the per-branch lockfiles an install
    /// under `mergeGitBranchLockfiles` has just folded into
    /// `pnpm-lock.yaml`. Leaving them behind would make the next install
    /// merge the same resolutions again.
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_CLEAN_GIT_BRANCH_LOCKFILES))]
    #[display("Failed to remove the git branch lockfiles: {_0}")]
    CleanGitBranchLockfiles(#[error(source)] std::io::Error),

    /// An entry could not be removed while the install was clearing the
    /// modules directory. `path` is that entry: the file or directory
    /// the user has to act on.
    // This variant and `ReadModulesDir` render their path through
    // `dunce::simplified` and `Display` rather than `{path:?}`: the
    // purge walks a canonicalized modules directory, so `Debug` would
    // print the verbatim prefix and escape every separator.
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_REMOVE_MODULES_DIR))]
    #[display(
        "Failed to remove {} from the modules directory: {error}",
        dunce::simplified(path).display()
    )]
    RemoveModulesDir {
        path: PathBuf,
        #[error(source)]
        error: std::io::Error,
    },

    /// The modules directory at `path` could not be listed, so the
    /// install cannot tell what is left in it. Shares the code of
    /// [`InstallError::RemoveModulesDir`] because both mean the same
    /// thing to a user: the modules directory could not be cleared.
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_REMOVE_MODULES_DIR))]
    #[display(
        "Failed to read the modules directory at {}: {error}",
        dunce::simplified(path).display()
    )]
    ReadModulesDir {
        path: PathBuf,
        #[error(source)]
        error: std::io::Error,
    },

    #[display(
        "Cannot safely repair the filtered install because the modules directory at {modules_dir:?} is outside the workspace root at {workspace_root:?}"
    )]
    #[diagnostic(code(pnpm_package_manager::unsafe_filtered_modules_dir))]
    UnsafeFilteredModulesDir { modules_dir: PathBuf, workspace_root: PathBuf },

    /// Surfaces a failure while removing the direct-dep links an
    /// `included` drift excluded — the non-destructive counterpart of
    /// the purge. See [`crate::prune_direct_deps_excluded_by_groups`].
    #[diagnostic(transparent)]
    PruneDirectDeps(#[error(source)] crate::PruneDirectDepsError),

    /// `pnpm-lock.yaml` doesn't match the on-disk `package.json` for
    /// the project being installed. `ERR_PNPM_OUTDATED_LOCKFILE`:
    /// the user (or CI) edited the manifest without regenerating the
    /// lockfile, and a frozen install would silently produce the
    /// wrong shape of `node_modules`. Fail the install instead.
    #[display(
        "Cannot install with \"frozen-lockfile\" because pnpm-lock.yaml is not up to date with package.json.\n\n  Failure reason:\n  {reason}"
    )]
    #[diagnostic(
        code(ERR_PNPM_OUTDATED_LOCKFILE),
        help(
            "Regenerate the lockfile with `pnpm install --lockfile-only` so that pnpm-lock.yaml reflects the current package.json, then re-run `pnpm install --frozen-lockfile`."
        )
    )]
    OutdatedLockfile { reason: StalenessReason },

    /// A setting the lockfile records no longer matches the one the
    /// current install resolved — `overrides`, `patchedDependencies`,
    /// `catalogs`, and the rest of pnpm's `getOutdatedLockfileSetting`
    /// set. Distinct from [`InstallError::OutdatedLockfile`], which is
    /// drift between the lockfile and `package.json`: naming the one
    /// setting that changed is more actionable than dumping the diff,
    /// and it is the code pnpm reports.
    #[display(
        r#"Cannot proceed with the frozen installation. The current "{setting}" configuration doesn't match the value found in the lockfile"#
    )]
    #[diagnostic(
        code(ERR_PNPM_LOCKFILE_CONFIG_MISMATCH),
        help(r#"Update your lockfile using "pnpm install --no-frozen-lockfile""#)
    )]
    LockfileConfigMismatch { setting: &'static str },

    /// `--frozen-lockfile` was requested against a lockfile whose
    /// `importers` map has no entry for the root project. Distinct
    /// from `NoLockfile` (file missing) — here the file exists but
    /// doesn't describe the project being installed.
    #[display(
        r#"Cannot install with "frozen-lockfile" because pnpm-lock.yaml has no `importers["{importer_id}"]` entry. Regenerate the lockfile with `pnpm install --lockfile-only`."#
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_NO_IMPORTER))]
    NoImporter { importer_id: String },

    /// Two flags that cannot both hold: a frozen install never rewrites
    /// `pnpm-lock.yaml`, which is the only thing `--update-checksums`
    /// does. Not to be confused with pnpm's
    /// `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE`, which is a
    /// stale lockfile under `--frozen-lockfile` and lives in
    /// `pnpm_env_installer`.
    #[display(
        "Cannot use --frozen-lockfile together with --update-checksums: frozen installs never rewrite pnpm-lock.yaml, but --update-checksums exists to do exactly that."
    )]
    #[diagnostic(code(ERR_PNPM_CONFIG_CONFLICT_FROZEN_LOCKFILE_WITH_UPDATE_CHECKSUMS))]
    FrozenLockfileWithUpdateChecksums,

    #[diagnostic(transparent)]
    FindWorkspaceDir(#[error(source)] pnpm_workspace::FindWorkspaceDirError),

    /// Reading `pnpm-workspace.yaml` to extract its `catalog` /
    /// `catalogs` sections failed.
    #[diagnostic(transparent)]
    ReadWorkspaceManifest(#[error(source)] pnpm_workspace::ReadWorkspaceManifestError),

    /// `pnpm-workspace.yaml` defined the `default` catalog twice
    /// (once via the top-level `catalog:` field and once via
    /// `catalogs.default`).
    #[diagnostic(transparent)]
    InvalidCatalogsConfiguration(#[error(source)] InvalidCatalogsConfigurationError),

    #[diagnostic(transparent)]
    CatalogResolution(#[error(source)] CatalogResolutionError),

    #[diagnostic(transparent)]
    FindWorkspaceProjects(#[error(source)] pnpm_workspace::FindWorkspaceProjectsError),

    /// `disallowWorkspaceCycles` and the projects this install covers
    /// depend on each other in a cycle.
    #[diagnostic(transparent)]
    CyclicWorkspaceDependencies(
        #[error(source)] crate::workspace_cycles::CyclicWorkspaceDependenciesError,
    ),

    /// Building the verifier list from config rejected a
    /// `minimumReleaseAgeExclude` or `trustPolicyExclude` pattern.
    /// The `INVALID_MINIMUM_RELEASE_AGE_EXCLUDE` /
    /// `INVALID_TRUST_POLICY_EXCLUDE` codes; the inner diagnostic
    /// carries the offending pattern.
    #[diagnostic(transparent)]
    BuildVerifiers(#[error(source)] BuildVerifiersError),

    /// The lockfile-verification gate rejected one or more lockfile
    /// entries — the lockfile contains versions weaker than the
    /// active `minimumReleaseAge` / `trustPolicy='no-downgrade'`
    /// policies allow. Transparent so the inner miette code
    /// (`MINIMUM_RELEASE_AGE_VIOLATION`, `TRUST_DOWNGRADE`,
    /// `LOCKFILE_RESOLUTION_VERIFICATION`) is what the user sees.
    #[diagnostic(transparent)]
    LockfileVerification(#[error(source)] VerifyError),

    /// Surfaces a failure to persist `.pnpm-workspace-state-v1.json`.
    /// Missing or unreadable state forces `pnpm run`'s
    /// `verifyDepsBeforeRun` check to fall back to "outdated", which
    /// is exactly the regression CI hits when pacquet runs the
    /// install — fail the install rather than letting a silent write
    /// error compound into spurious reinstalls.
    #[diagnostic(transparent)]
    WriteWorkspaceState(#[error(source)] UpdateWorkspaceStateError),

    /// Surfaces a failure to record the `allowBuilds` placeholders for the
    /// builds this install ignored. Fatal rather than silent: the install
    /// is about to tell the user to decide those builds, and a message
    /// pointing at a file that was never written is worse than no message.
    #[diagnostic(transparent)]
    ScaffoldAllowBuilds(
        #[error(source)] pnpm_workspace_manifest_writer::UpdateWorkspaceManifestError,
    ),

    /// Surfaces a failure from post-install pruning of policy and build
    /// entries in `pnpm-workspace.yaml`.
    #[diagnostic(transparent)]
    WriteWorkspaceManifest(#[error(source)] crate::catalog_cleanup::WriteWorkspaceCatalogsError),

    /// Surfaces a failure to persist `node_modules/.package-map.json`,
    /// the package-map metadata Node consumes when the user opts into
    /// `--experimental-package-map`.
    #[display("Failed to write node_modules/.package-map.json: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_WRITE_PACKAGE_MAP))]
    WritePackageMap(#[error(source)] crate::WritePackageMapError),

    /// A value in `pnpm.overrides` couldn't be parsed — the selector
    /// key isn't a recognizable package name, or the override value
    /// uses the `catalog:` protocol (which pacquet doesn't support
    /// yet). The `ERR_PNPM_INVALID_SELECTOR` and
    /// `ERR_PNPM_CATALOG_IN_OVERRIDES` codes.
    #[diagnostic(transparent)]
    InvalidOverrides(#[error(source)] pnpm_config_parse_overrides::ParseOverridesError),

    /// `--lockfile-only` was requested together with `lockfile: false`
    /// (pnpm's `useLockfile: false`). There is nothing left to do — the
    /// only output `--lockfile-only` produces is the lockfile, and that
    /// write is disabled — so the combination is a user-config conflict
    /// rather than a silent no-op. The
    /// `ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE` error.
    #[display("Cannot generate a pnpm-lock.yaml because lockfile is set to false")]
    #[diagnostic(code(ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE))]
    ConfigConflictLockfileOnlyWithNoLockfile,

    /// `--force` was requested together with `frozenStore`. Force
    /// re-imports packages into the store, which `frozenStore` opens
    /// read-only, so the combination cannot proceed. Mirrors pnpm's
    /// `ERR_PNPM_CONFIG_CONFLICT_FROZEN_STORE_WITH_FORCE`.
    #[display(
        "Cannot use force together with frozenStore: --force re-imports packages into the store, which is opened read-only when frozenStore is enabled"
    )]
    #[diagnostic(code(ERR_PNPM_CONFIG_CONFLICT_FROZEN_STORE_WITH_FORCE))]
    ConfigConflictFrozenStoreWithForce,

    /// `virtualStoreOnly` was requested with `enableModulesDir: false`
    /// while the global virtual store is off. The standard virtual
    /// store lives at `node_modules/.pnpm`, so suppressing
    /// `node_modules` leaves nowhere to populate. The global virtual
    /// store lives outside the project, which is why enabling it makes
    /// the same combination legal.
    #[display(
        "Cannot use virtualStoreOnly when enableModulesDir is false (the standard virtual store requires node_modules/.pnpm)"
    )]
    #[diagnostic(code(ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_ONLY_WITH_NO_MODULES_DIR))]
    ConfigConflictVirtualStoreOnlyWithNoModulesDir,
}
/// Hold back an [`InstallError::IgnoredBuilds`] verdict so the calling
/// command can finish writing `package.json` and `pnpm-workspace.yaml`
/// before it aborts: the install materialized the tree, and pnpm reports
/// the blocked builds only after both writes (`handleIgnoredBuilds` in
/// `installDeps`). The returned error is the caller's to raise once those
/// writes are done.
///
/// Every other error propagates straight away and leaves the manifests
/// untouched, matching pnpm — which throws those from inside the install
/// itself, before it reaches the writes.
pub fn defer_ignored_builds(
    outcome: Result<(), InstallError>,
) -> Result<Option<InstallError>, InstallError> {
    match outcome {
        Ok(()) => Ok(None),
        Err(error @ InstallError::IgnoredBuilds { .. }) => Ok(Some(error)),
        Err(error) => Err(error),
    }
}
