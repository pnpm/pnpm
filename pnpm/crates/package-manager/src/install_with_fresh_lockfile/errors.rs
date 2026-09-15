use crate::{
    CreateVirtualStoreError, DependenciesGraphToLockfileError, InstallPackageFromRegistryError,
    LinkRootComponentMembersError, LinkVirtualStoreBinsError, SymlinkDirectDependenciesError,
    VersionPolicyError,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_cmd_shim::LinkBinsError;
use pnpm_lockfile::SaveLockfileError;
use pnpm_resolving_deps_resolver::ResolveDependencyTreeError;
use pnpm_resolving_npm_resolver::MergeNamedRegistriesError;

/// Error type of [`InstallWithFreshLockfile`](crate::InstallWithFreshLockfile).
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallWithFreshLockfileError {
    /// A path named by the `pnpmfile` setting is not on disk. pnpm reports the
    /// same code and message from `requireHooks`.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_NOT_FOUND))]
    MissingPnpmfile(#[error(not(source))] pnpm_hooks::finder::MissingPnpmfileError),
    /// The concurrent pre-resolve verification of the existing lockfile
    /// rejected it. The orchestrator maps this back to
    /// `InstallError::LockfileVerification` so the failure keeps the
    /// shape of the eager gates.
    #[diagnostic(transparent)]
    LockfileVerification(#[error(source)] pnpm_lockfile_verification::VerifyError),

    #[diagnostic(transparent)]
    InstallPackageFromRegistry(#[error(source)] InstallPackageFromRegistryError),

    #[diagnostic(transparent)]
    CreateVirtualStore(#[error(source)] CreateVirtualStoreError),

    #[diagnostic(transparent)]
    SymlinkDirectDependencies(#[error(source)] SymlinkDirectDependenciesError),

    /// Surfaces a failure while removing stale direct-dep or hoist
    /// links during the pre-link reconciliation pass.
    #[diagnostic(transparent)]
    PruneStaleModules(#[error(source)] crate::PruneDirectDepsError),

    #[diagnostic(transparent)]
    LinkPhase(#[error(source)] pnpm_deps_restorer::linking::LinkPhaseError),

    /// Surfaces a failure to cross-link a Bit root component's injected
    /// members into one another's virtual-store slot. Only reachable
    /// when an importer manifest declares
    /// `installConfig.hoistingLimits: "workspaces"`.
    #[diagnostic(transparent)]
    LinkRootComponentMembers(#[error(source)] LinkRootComponentMembersError),

    /// Surfaces failures from [`crate::lockfile_to_hoisted_dep_graph`]
    /// when a fresh install runs under `nodeLinker: hoisted`. Same
    /// shape the frozen-lockfile path surfaces — see
    /// `InstallFrozenLockfileError::HoistedDepGraph`.
    #[diagnostic(transparent)]
    HoistedDepGraph(#[error(source)] crate::HoistedDepGraphError),

    /// Surfaces failures from [`crate::link_hoisted_modules()`] while
    /// materializing the on-disk hoisted tree on the fresh path. Same
    /// shape the frozen-lockfile path surfaces — see
    /// `InstallFrozenLockfileError::LinkHoistedModules`.
    #[diagnostic(transparent)]
    LinkHoistedModules(#[error(source)] crate::LinkHoistedModulesError),

    #[display("failed to write package map: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_WRITE_PACKAGE_MAP))]
    WritePackageMap(#[error(source)] crate::WritePackageMapError),

    #[display("failed to write PnP loader: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_WRITE_PNP_FILE))]
    WritePnpFile(#[error(source)] crate::WritePnpFileError),

    #[diagnostic(transparent)]
    LinkBins(#[error(source)] LinkBinsError),

    /// Surfaces a failure to create one of the hoist symlinks
    /// (`<private_hoisted_modules_dir>/<alias>` or
    /// `<public_hoisted_modules_dir>/<alias>`). EEXIST is
    /// already swallowed by the hoist helper, so this only fires
    /// on real I/O failures.
    #[diagnostic(transparent)]
    HoistSymlink(#[error(source)] crate::SymlinkPackageError),

    /// Surfaces a failure to link bins of privately-hoisted aliases
    /// into the virtual-store-local `<vs>/node_modules/.bin`.
    #[diagnostic(transparent)]
    HoistLinkBins(#[error(source)] LinkBinsError),

    #[diagnostic(transparent)]
    LinkVirtualStoreBins(#[error(source)] LinkVirtualStoreBinsError),

    /// The resolver chain failed for at least one dependency. The
    /// diagnostic is forwarded transparently so a canonical inner code
    /// (e.g. a traversal name's `ERR_PNPM_INVALID_DEPENDENCY_NAME`)
    /// reaches the CLI unchanged. The `Display` still interpolates the
    /// inner error so consumers that stringify the top-level error
    /// (e.g. pnpr's `resolve.rs`, which forwards `err.to_string()` over
    /// the wire) keep the detail.
    #[display("Failed to resolve dependency tree: {_0}")]
    #[diagnostic(transparent)]
    ResolveDependencyTree(#[error(source)] ResolveDependencyTreeError),

    /// Surfaces a failure to read the manifest of a workspace-root
    /// `link:` / `file:` dependency, whose version stands in for the peer
    /// it may satisfy under `resolvePeersFromWorkspaceRoot`.
    #[display("Failed to read the manifest of a workspace root dependency: {_0}")]
    #[diagnostic(transparent)]
    RootDepManifest(#[error(source)] pnpm_package_manifest::PackageManifestError),

    #[display("Failed to build lockfile from resolved dependency graph: {_0}")]
    #[diagnostic(code(pnpm_package_manager::dependencies_graph_to_lockfile))]
    DependenciesGraphToLockfile(#[error(source)] Box<DependenciesGraphToLockfileError>),

    /// `minimumReleaseAgeExclude` patterns rejected at compile time.
    /// Surfaced as `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE`.
    #[display("Invalid value in minimumReleaseAgeExclude: {_0}")]
    #[diagnostic(code(ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE))]
    MinimumReleaseAgeExclude(#[error(source)] pnpm_config::version_policy::VersionPolicyError),

    /// `trustPolicyExclude` patterns rejected at compile time.
    /// Surfaced as `ERR_PNPM_INVALID_TRUST_POLICY_EXCLUDE`.
    #[display("Invalid value in trustPolicyExclude: {_0}")]
    #[diagnostic(code(ERR_PNPM_INVALID_TRUST_POLICY_EXCLUDE))]
    TrustPolicyExclude(#[error(source)] pnpm_config::version_policy::VersionPolicyError),

    /// `allowBuilds` patterns in `pnpm-workspace.yaml` couldn't be
    /// parsed. Same `VersionPolicyError` shape the frozen-lockfile
    /// path surfaces — see `InstallFrozenLockfileError::VersionPolicy`.
    #[diagnostic(transparent)]
    AllowBuildsPolicy(#[error(source)] VersionPolicyError),

    /// Surfaces any failure from the shared lifecycle-script build
    /// phase — `patchedDependencies` resolution, the `BuildModules`
    /// run, or the post-build top-level bin link. Shared with the
    /// frozen-lockfile path via `run_build_phase`.
    #[diagnostic(transparent)]
    BuildPhase(#[error(source)] crate::install_frozen_lockfile::BuildPhaseError),

    #[diagnostic(transparent)]
    MinimumReleaseAge(#[error(source)] crate::minimum_release_age::MinimumReleaseAgeError),

    /// Surfaces any failure from the fresh-lockfile installability
    /// pass before virtual-store materialization starts.
    #[diagnostic(transparent)]
    Installability(#[error(source)] Box<pnpm_package_is_installable::InstallabilityError>),

    #[diagnostic(transparent)]
    MergeFilteredWantedLockfile(#[error(source)] crate::MergeFilteredWantedLockfileError),

    /// Failed to resolve and hash `patchedDependencies` against the
    /// workspace directory.
    #[diagnostic(transparent)]
    ResolvePatchedDependencies(#[error(source)] pnpm_patching::ResolvePatchedDependenciesError),

    /// Failed to read or hash a patch file when computing the
    /// lockfile's top-level `patchedDependencies` block.
    #[diagnostic(transparent)]
    CalcPatchHashes(#[error(source)] pnpm_patching::CalcPatchHashError),

    /// One or more configured patches were never applied because no
    /// package matched their key. Surfaced as `ERR_PNPM_UNUSED_PATCH`
    /// unless `allowUnusedPatches` is `true`.
    #[diagnostic(transparent)]
    UnusedPatch(#[error(source)] pnpm_patching::UnusedPatchError),

    /// A user-defined `namedRegistries` entry mapped an alias to a
    /// non-http(s) URL. Surfaced at resolver construction so the
    /// install fails fast with a specific error code instead of a
    /// downstream 404. Surfaced as
    /// `ERR_PNPM_INVALID_NAMED_REGISTRY_URL`.
    #[diagnostic(transparent)]
    InvalidNamedRegistry(#[error(source)] MergeNamedRegistriesError),

    /// A `packageExtensions` selector's `@<range>` half failed to
    /// parse as a `node-semver` range. A malformed range is rejected
    /// at install start, not at the first per-manifest match, so the
    /// user sees the bad selector before any tarballs are fetched.
    #[diagnostic(transparent)]
    InvalidPackageExtensionSelector(
        #[error(source)] crate::package_extender::InvalidPackageExtensionSelector,
    ),

    /// A value in `pnpm.overrides` couldn't be parsed before the
    /// fresh resolver's read-package hook was built.
    #[diagnostic(transparent)]
    InvalidOverrides(#[error(source)] pnpm_config_parse_overrides::ParseOverridesError),

    /// The first writer of a shared `(name, version)` slot dropped its
    /// completion signal without sending `true`. In practice this only
    /// fires when the first writer's task panicked / was cancelled
    /// mid-import; a second visitor that was waiting on the slot can't
    /// safely create its per-parent symlink (the virtual-store target
    /// directory may not exist), so the install fails closed.
    #[display(
        "First writer for virtual-store slot {virtual_store_name} dropped before signalling completion"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_FIRST_WRITER_ABORTED))]
    FirstWriterAborted {
        #[error(not(source))]
        virtual_store_name: String,
    },

    /// Persisting the freshly-resolved `pnpm-lock.yaml` failed. Surfaced
    /// rather than swallowed because a missing wanted lockfile would
    /// force the next install to re-resolve every dep and would break
    /// the `pnpm install --frozen-lockfile` headless path.
    #[diagnostic(transparent)]
    SaveWantedLockfile(#[error(source)] SaveLockfileError),

    /// The `afterAllResolved` pnpmfile hook threw or otherwise failed.
    /// A throwing `afterAllResolved` aborts the install.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    AfterAllResolvedHook(#[error(not(source))] pnpm_hooks::HookError),

    /// The freshly-built lockfile could not be serialized to JSON to pass to
    /// the `afterAllResolved` pnpmfile hook.
    #[display("Failed to serialize lockfile for the afterAllResolved hook: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_AFTER_ALL_RESOLVED_SERIALIZE))]
    AfterAllResolvedSerialize(#[error(source)] serde_json::Error),

    /// The pnpmfile's `getCustomResolvers` hook threw while loading custom
    /// resolvers. A throwing custom-resolver hook aborts the install.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    CustomResolverHook(#[error(not(source))] pnpm_hooks::HookError),

    /// The pnpmfile threw while loading its custom `fetchers` export.
    /// Same fatality rule as [`Self::CustomResolverHook`] and the
    /// frozen-lockfile path's custom-fetcher load.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    CustomFetcherHook(#[error(not(source))] pnpm_hooks::HookError),

    /// A custom resolver's `shouldRefreshResolution` hook threw while
    /// checking whether to force re-resolution. A throwing hook aborts
    /// the install.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    CustomResolverForceResolve(#[error(not(source))] pnpm_hooks::HookError),
}
impl From<crate::install_frozen_lockfile::HoistedLinkerError> for InstallWithFreshLockfileError {
    fn from(error: crate::install_frozen_lockfile::HoistedLinkerError) -> Self {
        use crate::install_frozen_lockfile::HoistedLinkerError;
        match error {
            HoistedLinkerError::HoistedDepGraph(error) => {
                InstallWithFreshLockfileError::HoistedDepGraph(error)
            }
            HoistedLinkerError::LinkHoistedModules(error) => {
                InstallWithFreshLockfileError::LinkHoistedModules(error)
            }
            HoistedLinkerError::SymlinkDirectDependencies(error) => {
                InstallWithFreshLockfileError::SymlinkDirectDependencies(error)
            }
            HoistedLinkerError::WritePackageMap(error) => {
                InstallWithFreshLockfileError::WritePackageMap(error)
            }
        }
    }
}
