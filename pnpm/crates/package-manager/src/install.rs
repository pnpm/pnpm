use crate::{
    BuildVerifiersError, HoistedDependencies, InstallFrozenLockfile, InstallFrozenLockfileError,
    InstallWithFreshLockfile, InstallWithFreshLockfileError, LockfileVerificationOverride,
    OptimisticRepeatInstallCheck, RebuildOptions, ResolvedPackages, UpdateSeedPolicy,
    build_resolution_verifiers, check_optimistic_repeat_install, emit_initial_package_manifest,
    link_project_bins, optimistic_repeat_install::Decision as OptimisticRepeatInstallDecision,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_config::{
    InvalidCatalogsConfigurationError, get_catalogs_from_workspace_manifest,
};
use pacquet_catalogs_types::Catalogs;
use pacquet_cmd_shim::LinkBinsError;
use pacquet_config::{Config, NodeLinker, PNPM_VERSION};
use pacquet_executor::{
    LifecycleScriptError, RunPostinstallHooks,
    ScriptsPrependNodePath as ExecScriptsPrependNodePath, run_project_lifecycle_scripts,
};
use pacquet_lockfile::{
    LazyLockfile, LoadLockfileError, Lockfile, MaybeLazyLockfile, SaveLockfileError,
    StalenessReason, VersionPart, satisfies_package_manifest,
};
use pacquet_lockfile_verification::{
    VerifyError, VerifyLockfileResolutionsOptions, record_lockfile_verified,
    verify_lockfile_resolutions,
};
use pacquet_modules_yaml::{
    Host, IncludedDependencies, LayoutVersion, Modules, NodeLinker as ModulesNodeLinker,
    ReadModulesError, WriteModulesError, write_modules_manifest,
};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_package_manifest::{
    DependencyGroup, PackageManifest, node_version_from_engines_runtime,
};
use pacquet_reporter::{
    ContextLog, LogEvent, LogLevel, PnpmLog, Reporter, Stage, StageLog, SummaryLog,
};
use pacquet_resolving_npm_resolver::InMemoryPackageMetaCache;
use pacquet_resolving_resolver_base::ResolutionVerifier;
use pacquet_tarball::MemCache;
use pacquet_workspace_state::{
    ProjectEntry, UpdateWorkspaceStateError, WorkspaceState, now_millis, update_workspace_state,
};
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::IsTerminal,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU8},
    time::SystemTime,
};

/// Run the lockfile verification fan-out to completion, blocking the
/// caller on the verdict. Used by the install paths that have no fetch
/// to overlap verification with (fresh resolve, the lockfile-only and
/// up-to-date short-circuits); the frozen materialization path instead
/// runs verification concurrently with the fetch inside
/// [`InstallFrozenLockfile`]. A no-op when `verifiers` is empty.
async fn verify_lockfile_eagerly<Reporter: pacquet_reporter::Reporter>(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    lockfile_path: Option<&Path>,
    cache_dir: &Path,
) -> Result<(), InstallError> {
    if verifiers.is_empty() {
        return Ok(());
    }
    verify_lockfile_resolutions::<Reporter>(
        lockfile,
        verifiers,
        &VerifyLockfileResolutionsOptions {
            concurrency: None,
            lockfile_path,
            cache_dir: Some(cache_dir),
        },
    )
    .await
    .map_err(InstallError::LockfileVerification)
}

fn map_frozen_lockfile_error(error: InstallFrozenLockfileError) -> InstallError {
    match error {
        InstallFrozenLockfileError::LockfileVerification(verify_error) => {
            InstallError::LockfileVerification(verify_error)
        }
        other => InstallError::FrozenLockfile(other),
    }
}

/// Shared out-map for [`Install::peer_issues_sink`]: importer id →
/// that importer's peer-dependency issues from the fresh resolve.
pub type PeerIssuesSink = Arc<
    std::sync::Mutex<
        std::collections::BTreeMap<String, pacquet_resolving_deps_resolver::PeerDependencyIssues>,
    >,
>;

pub struct WorkspaceInstallSelection<'a> {
    pub all_projects: &'a [pacquet_workspace::Project],
    pub ordered_groups: &'a [Vec<PathBuf>],
    pub ordered_dirs: &'a [PathBuf],
    pub selected_dirs: &'a HashSet<PathBuf>,
    pub active_manifest_is_standin: bool,
}

pub(crate) fn selected_project_indices(
    projects: &[pacquet_workspace::Project],
    ordered_dirs: &[PathBuf],
    selected_dirs: &HashSet<PathBuf>,
) -> Vec<usize> {
    let project_indices = projects
        .iter()
        .enumerate()
        .map(|(index, project)| (project.root_dir.as_path(), index))
        .collect::<std::collections::HashMap<_, _>>();
    let mut seen_dirs = HashSet::with_capacity(selected_dirs.len());
    let indices = ordered_dirs
        .iter()
        .filter(|dir| selected_dirs.contains(*dir))
        .map(|dir| {
            assert!(seen_dirs.insert(dir.as_path()), "selected project must be ordered once");
            *project_indices.get(dir.as_path()).expect("every selected project must be discovered")
        })
        .collect::<Vec<_>>();
    assert_eq!(seen_dirs.len(), selected_dirs.len(), "every selected project must be ordered");
    indices
}

/// This subroutine does everything `pacquet install` is supposed to do.
#[must_use]
pub struct Install<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    /// Shared in-memory tarball cache. Held behind [`Arc`] so the
    /// prefetcher constructed in [`InstallWithFreshLockfile::run`]
    /// can capture an owned clone into the background download task
    /// while the install-side calls still take `&MemCache` via deref.
    pub tarball_mem_cache: Arc<MemCache>,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    /// Same client behind an [`Arc`] for the lockfile-verification
    /// gate (which owns its `ThrottledClient` to outlive the
    /// per-call lifetime of [`Self::http_client`]). The CLI builds
    /// both from a single source; the duplicate is the smallest
    /// change that bridges the borrowed `&` shape every existing
    /// sub-installer expects with the owned `Arc` the verifier
    /// needs.
    pub http_client_arc: Arc<ThrottledClient>,
    pub config: &'static Config,
    pub manifest: &'a PackageManifest,
    /// Emit `pnpm:package-manifest initial` from this install run.
    /// Partial mutations that need the pre-mutation manifest snapshot
    /// emit it before changing the manifest and pass `false` here.
    pub emit_initial_manifest: bool,
    pub lockfile: MaybeLazyLockfile<'a>,
    /// Absolute path of the loaded `pnpm-lock.yaml`. Threaded into
    /// the lockfile-verification gate so the per-path stat shortcut
    /// in `<cache_dir>/lockfile-verified.jsonl` can fire on repeat
    /// installs, and into the `pnpm:lockfile-verification` reporter
    /// payload. `None` disables the cache for this run (every call
    /// re-verifies) and falls back to deriving the path from
    /// `workspace_root`.
    pub lockfile_path: Option<&'a Path>,
    pub dependency_groups: DependencyGroupList,
    pub frozen_lockfile: bool,
    /// `preferFrozenLockfile` value to honor for *this* invocation.
    /// `None` (no CLI flag) means "use `config.prefer_frozen_lockfile`";
    /// `Some(true)` forces the auto-frozen fast path on, `Some(false)`
    /// forces it off. Computed at the CLI layer from the
    /// `--prefer-frozen-lockfile` / `--no-prefer-frozen-lockfile`
    /// flags. Threaded as an [`Option<bool>`] so the dispatch can
    /// tell a per-invocation override apart from the config default.
    pub prefer_frozen_lockfile: Option<bool>,
    /// Skip the per-importer `package.json` ↔ `pnpm-lock.yaml`
    /// freshness check ([`satisfies_package_manifest`]) that
    /// normally guards `--frozen-lockfile`. Surfaced as
    /// `--ignore-manifest-check` on the CLI; intended for the
    /// `configDependencies` delegation path, where the lockfile has
    /// just been resolved and written but the updated manifest hasn't
    /// been written yet. Settings-drift checks (`overrides`,
    /// `ignoredOptionalDependencies`, ...) still run — they don't
    /// inspect the manifest and the bug this flag addresses is
    /// specifically the per-dep specifier mismatch.
    pub ignore_manifest_check: bool,
    /// When `true`, runtime dependencies (`node@runtime:` /
    /// `deno@runtime:` / `bun@runtime:`) are skipped — their
    /// archives aren't fetched, their slots aren't materialized,
    /// and their bins aren't linked. Computed at the CLI layer
    /// from `config.skip_runtimes || --no-runtime`. The rest of
    /// the install proceeds normally. See
    /// `pacquet_config::Config::skip_runtimes`.
    pub skip_runtimes: bool,
    /// Effective `trustLockfile` value for *this* invocation. The CLI
    /// layer ORs the `--trust-lockfile` flag with `config.trust_lockfile`
    /// so a yaml `true` can't be overridden back to `false` from the
    /// CLI — the same stance applied to similar flags. Threaded as a
    /// separate field for the same reason [`Self::skip_runtimes`] is:
    /// `state.config` is a shared `&'static Config`, so the CLI
    /// override merge happens in the caller and lands here as a
    /// fully-resolved value.
    pub trust_lockfile: bool,
    /// The `--update-checksums` flag: refresh locked integrity values
    /// from the registry. Skips the frozen-lockfile path so the
    /// fresh-resolve path rewrites them.
    pub update_checksums: bool,
    /// Whether this is a full project install (`pacquet install`,
    /// pnpm's `mutation: 'install'`) rather than a partial one
    /// (`pacquet add`, pnpm's `mutation: 'installSome'`). Gates the
    /// project's own lifecycle scripts: they run only for the full
    /// install via the `mutation === 'install'` filter, so a named
    /// install such as `pacquet add foo` does not fire the root
    /// project's preinstall/postinstall/prepare/etc.
    pub is_full_install: bool,
    /// Whether every mutation this run performs is a plain install
    /// (upstream's `installsOnly`, true for `pacquet install` /
    /// `pacquet update`). A plain install may recreate a modules
    /// directory whose layout settings drifted; `add` / `remove` set
    /// this `false` and fail with the upstream `*_DIFF` errors
    /// instead — pnpm's `validateModules` contract. Distinct from
    /// [`Self::is_full_install`], which stays `false` for a named
    /// `update`.
    pub installs_only: bool,
    /// `supportedArchitectures` after merging
    /// `Config::supported_architectures` from `pnpm-workspace.yaml`
    /// with the CLI per-axis overrides (`--cpu` / `--os` / `--libc`).
    /// Threaded into `InstallabilityHost` in the frozen-lockfile
    /// path so optional platform-tagged dependencies for the listed
    /// triples are kept even when they don't match the host. `None`
    /// means "host triple is the sole accept set" — the behavior
    /// when neither yaml nor CLI sets a value.
    ///
    /// Computed at the CLI layer (see
    /// `pacquet_cli::cli_args::supported_architectures::SupportedArchitecturesArgs`)
    /// instead of being read from `config` directly, because
    /// `State.config` is a shared `&'static Config` — the CLI
    /// override merge happens in the caller and lands here as a
    /// fully-resolved value.
    pub supported_architectures: Option<pacquet_package_is_installable::SupportedArchitectures>,
    /// `nodeLinker` value to honor for *this* invocation. The CLI
    /// layer applies any `--node-linker` override here; absent a
    /// flag, this equals `config.node_linker`. Threaded as a
    /// separate field for the same reason
    /// [`Self::supported_architectures`] is: `state.config` is a
    /// shared `&'static Config`, so the CLI override merge happens
    /// in the caller and lands here as a fully-resolved value.
    /// Used today for the `.modules.yaml.nodeLinker` write and
    /// (in Slice 6) for the install-pipeline branch.
    pub node_linker: pacquet_config::NodeLinker,
    /// When `true`, resolve dependencies and (re)write `pnpm-lock.yaml`
    /// but skip every materialization step: no tarball is fetched into
    /// the store, no `node_modules` is linked, and neither
    /// `.modules.yaml` nor the current lockfile
    /// (`<virtual_store_dir>/lock.yaml`) nor the workspace-state file
    /// is written. Surfaced as `--lockfile-only` on the CLI. A pure
    /// per-invocation flag (no `pnpm-workspace.yaml` / `config.yaml`
    /// counterpart — `lockfile-only` is an excluded config key),
    /// so it is threaded straight from the CLI like
    /// [`Self::frozen_lockfile`]. Equivalent to npm's
    /// `--package-lock-only`.
    pub lockfile_only: bool,
    /// `--dry-run`: resolve fully but write nothing, then report what a
    /// real install would change. Forces the fresh-resolve path (so the
    /// would-be lockfile is always computed), suppresses every write —
    /// `pnpm-lock.yaml`, `node_modules`, `.modules.yaml`, the current
    /// lockfile, the workspace-state file — and exits 0 regardless of
    /// whether changes were found.
    pub dry_run: bool,
    /// Which lockfile pins to withhold from the preferred-versions seed.
    /// [`UpdateSeedPolicy::KeepAll`] for `install` / `add`; the `DropAll`
    /// / `DropOnly` variants drive `pacquet update`'s compatible bump by
    /// forcing the affected names to re-resolve to highest-in-range.
    /// Forwarded to [`InstallWithFreshLockfile`]; ignored on the frozen
    /// path (`update` always takes the fresh-resolve path). When set to
    /// anything other than `KeepAll` the optimistic repeat-install
    /// short-circuit is also bypassed so an `update` that finds newer
    /// in-range versions isn't skipped as "already up to date".
    pub update_seed_policy: UpdateSeedPolicy,
    /// Per-invocation `Authorization`-header override for resolve/verify;
    /// `None` (every local install) uses `config.auth_headers`. The pnpr
    /// resolver threads request-scoped [`AuthHeaders`] here so it
    /// resolves a caller's private content without baking per-user auth
    /// into the shared `&'static Config`.
    pub auth_override: Option<Arc<AuthHeaders>>,
    /// Sink notified for each resolved tarball package as the fresh
    /// resolve yields it. `None` for every local install. The pnpr
    /// server installs one to stream fetch frames to the client so
    /// tarball downloads overlap server-side resolution.
    /// Ignored on the frozen path (no tree walk to observe).
    pub resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
    /// Out-channel for the fresh resolve's per-importer peer-dependency
    /// issues. `None` for every CLI install (issues are only logged).
    /// The napi `getPeerDependencyIssues` runs a `dry_run` install with
    /// a sink to collect them — and a sink-driven dry run suppresses
    /// the CLI's stdout diff report, since it is a programmatic query
    /// rather than an `--dry-run` preview. Only the fresh path fills
    /// it (the frozen path resolves nothing).
    pub peer_issues_sink: Option<crate::PeerIssuesSink>,
    /// In-memory catalogs to resolve against instead of reading
    /// `pnpm-workspace.yaml` from disk. `None` (every plain install) reads
    /// the workspace manifest. `pacquet update` sets this so a `--latest`
    /// catalog bump drives resolution even under `--no-save`, where the
    /// bumped entry is intentionally not persisted to disk.
    pub catalogs_override: Option<Catalogs>,
    /// When `true`, the optimistic repeat-install fast path is
    /// disabled so the full install pipeline always runs. `pacquet
    /// prune` sets this because the fast path short-circuits before
    /// the virtual-store sweep, meaning extraneous packages can
    /// survive a prune when the lockfile hasn't changed.
    pub disable_optimistic_repeat_install: bool,
    /// In-process `readPackage` / `afterAllResolved` hooks supplied by an
    /// embedder (the Node API binding) instead of a `.pnpmfile.cjs` on disk.
    /// `Some` replaces the disk lookup on the fresh-resolve path entirely;
    /// `None` (every CLI install) falls back to `finder::load_pnpmfile`.
    /// Ignored on the frozen path, which performs no resolution.
    pub pnpmfile_hook_override: Option<Arc<dyn pacquet_hooks::PnpmfileHooks>>,
    /// Workspace importers supplied in memory by an embedder (the Node API
    /// binding) instead of discovering them from a `pnpm-workspace.yaml` on
    /// disk. `Some` bypasses the on-disk workspace-project walk entirely — the
    /// root importer still comes from [`Self::manifest`], siblings from this
    /// list. `None` (every CLI install) walks the workspace on disk.
    pub workspace_projects_override: Option<Vec<pacquet_workspace::Project>>,
}

/// Error type of [`Install`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallError {
    #[display(
        "Headless installation requires a pnpm-lock.yaml file, but none was found. Run `pnpm install` without --frozen-lockfile to create one."
    )]
    #[diagnostic(code(ERR_PNPM_NO_LOCKFILE))]
    NoLockfile,

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

    /// A custom resolver hook failed (loading the pnpmfile's resolvers
    /// or running `shouldRefreshResolution`) while deciding whether the
    /// frozen-path optimization may run. A throwing hook aborts the
    /// install.
    #[display("{_0}")]
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    CustomResolverForceResolve(#[error(not(source))] pacquet_hooks::HookError),

    #[diagnostic(transparent)]
    FrozenLockfile(#[error(source)] InstallFrozenLockfileError),

    /// A workspace project's own lifecycle script
    /// (preinstall/install/postinstall/preprepare/prepare/postprepare)
    /// exited non-zero. Unlike a dependency build failure — which
    /// `BuildModules` can swallow for optional deps — a project script
    /// failure always fails the install, matching pnpm.
    #[diagnostic(transparent)]
    ProjectLifecycleScript(#[error(source)] LifecycleScriptError),

    #[diagnostic(transparent)]
    ProjectBinLink(#[error(source)] LinkBinsError),

    #[display("Failed to create the workspace lifecycle thread pool: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_LIFECYCLE_THREAD_POOL))]
    ProjectLifecycleThreadPool(#[error(source)] rayon::ThreadPoolBuildError),

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
    /// passed on the install (see [`MaybeLazyLockfile`]).
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

    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_REMOVE_MODULES_DIR))]
    #[display("Failed to remove modules directory contents: {_0}")]
    RemoveModulesDir(#[error(source)] std::io::Error),

    #[display(
        "Cannot safely repair the filtered install because the modules directory at {modules_dir:?} is outside the workspace root at {workspace_root:?}"
    )]
    #[diagnostic(code(pacquet_package_manager::unsafe_filtered_modules_dir))]
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
    /// `pacquet_env_installer`.
    #[display(
        "Cannot use --frozen-lockfile together with --update-checksums: frozen installs never rewrite pnpm-lock.yaml, but --update-checksums exists to do exactly that."
    )]
    #[diagnostic(code(ERR_PNPM_CONFIG_CONFLICT_FROZEN_LOCKFILE_WITH_UPDATE_CHECKSUMS))]
    FrozenLockfileWithUpdateChecksums,

    #[diagnostic(transparent)]
    FindWorkspaceDir(#[error(source)] pacquet_workspace::FindWorkspaceDirError),

    /// Reading `pnpm-workspace.yaml` to extract its `catalog` /
    /// `catalogs` sections failed.
    #[diagnostic(transparent)]
    ReadWorkspaceManifest(#[error(source)] pacquet_workspace::ReadWorkspaceManifestError),

    /// `pnpm-workspace.yaml` defined the `default` catalog twice
    /// (once via the top-level `catalog:` field and once via
    /// `catalogs.default`).
    #[diagnostic(transparent)]
    InvalidCatalogsConfiguration(#[error(source)] InvalidCatalogsConfigurationError),

    #[diagnostic(transparent)]
    FindWorkspaceProjects(#[error(source)] pacquet_workspace::FindWorkspaceProjectsError),

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
    InvalidOverrides(#[error(source)] pacquet_config_parse_overrides::ParseOverridesError),

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

#[derive(Default)]
struct InstallRunOptions<'install, 'selection> {
    lockfile_verification_override: Option<LockfileVerificationOverride<'install>>,
    rebuild: Option<RebuildOptions>,
    selection: Option<WorkspaceInstallSelection<'selection>>,
    root_manifest_as_workspace_root: bool,
    /// Forces the interactive-prompt eligibility that is otherwise derived
    /// from the process environment, so tests can exercise both branches.
    prompt_eligibility_override: Option<bool>,
}

impl<'a, DependencyGroupList> Install<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    /// Execute the subroutine.
    pub async fn run<Reporter: self::Reporter + 'static>(self) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions::default())).await
    }

    #[cfg(test)]
    pub(crate) async fn run_with_prompt_eligibility<Reporter: self::Reporter + 'static>(
        self,
        can_prompt: bool,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            prompt_eligibility_override: Some(can_prompt),
            ..Default::default()
        }))
        .await
    }

    pub async fn run_with_lockfile_verification<Reporter: self::Reporter + 'static>(
        self,
        lockfile_verification_override: LockfileVerificationOverride<'a>,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            lockfile_verification_override: Some(lockfile_verification_override),
            ..Default::default()
        }))
        .await
    }

    pub async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        selection: WorkspaceInstallSelection<'_>,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            selection: Some(selection),
            ..Default::default()
        }))
        .await
    }

    pub async fn run_selected_with_lockfile_verification<Reporter: self::Reporter + 'static>(
        self,
        selection: WorkspaceInstallSelection<'_>,
        lockfile_verification_override: LockfileVerificationOverride<'a>,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            lockfile_verification_override: Some(lockfile_verification_override),
            selection: Some(selection),
            ..Default::default()
        }))
        .await
    }

    /// Execute with the active manifest mapped to the root importer while
    /// retaining workspace discovery for `workspace:` dependency resolution.
    pub async fn run_with_root_importer<Reporter: self::Reporter + 'static>(
        self,
    ) -> Result<(), InstallError> {
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            root_manifest_as_workspace_root: true,
            ..Default::default()
        }))
        .await
    }

    /// Execute as a forced rebuild: take the frozen path against the
    /// already-resolved lockfile + materialized `node_modules`, bypass the
    /// "up to date" short-circuit, and re-run the lifecycle scripts of the
    /// selected packages (or every build-needing package when
    /// `rebuild.selected_names` is `None`). Drives `pacquet rebuild` and
    /// the rebuild step of `pacquet approve-builds`.
    ///
    /// # Panics
    ///
    /// Panics unless `frozen_lockfile` is set: a rebuild must take the
    /// frozen path, since the fresh-resolve path drops the rebuild
    /// selection and would silently degrade to a plain install.
    pub async fn run_rebuild<Reporter: self::Reporter + 'static>(
        self,
        rebuild: RebuildOptions,
    ) -> Result<(), InstallError> {
        assert!(self.frozen_lockfile, "run_rebuild requires frozen_lockfile = true");
        Box::pin(self.run_inner::<Reporter>(InstallRunOptions {
            rebuild: Some(rebuild),
            ..Default::default()
        }))
        .await
    }

    async fn run_inner<Reporter: self::Reporter + 'static>(
        self,
        options: InstallRunOptions<'a, '_>,
    ) -> Result<(), InstallError> {
        let InstallRunOptions {
            lockfile_verification_override,
            rebuild,
            selection,
            root_manifest_as_workspace_root,
            prompt_eligibility_override,
        } = options;
        let Install {
            tarball_mem_cache,
            resolved_packages,
            http_client,
            http_client_arc,
            config,
            manifest,
            emit_initial_manifest,
            lockfile,
            lockfile_path,
            dependency_groups,
            frozen_lockfile,
            prefer_frozen_lockfile,
            ignore_manifest_check,
            skip_runtimes,
            trust_lockfile,
            update_checksums,
            is_full_install,
            installs_only,
            supported_architectures,
            node_linker,
            lockfile_only,
            dry_run,
            update_seed_policy,
            auth_override,
            resolution_observer,
            peer_issues_sink,
            catalogs_override,
            disable_optimistic_repeat_install,
            pnpmfile_hook_override,
            workspace_projects_override,
        } = self;
        let can_prompt = prompt_eligibility_override
            .unwrap_or_else(|| !is_ci::cached() && std::io::stdin().is_terminal());
        // Read before the sink is moved into the fresh-path inputs.
        let peer_issues_sink_is_none = peer_issues_sink.is_none();

        // `--lockfile-only` with `lockfile: false` (pnpm's
        // `useLockfile: false`) is a config conflict: the only output the
        // flag produces is the lockfile, and that write is disabled.
        // Fail fast rather than run a resolve that writes nothing.
        if lockfile_only && !config.lockfile {
            return Err(InstallError::ConfigConflictLockfileOnlyWithNoLockfile);
        }

        // `enableModulesDir: false` (with the global virtual store off) is
        // "resolve and write the lockfile, materialize nothing" — the same
        // pipeline `--lockfile-only` takes, entered from config. It stays
        // outside the `lockfile: false` conflict above (pnpm accepts that
        // combination and simply writes nothing), and never turns a
        // rebuild — which runs against an already-materialized
        // `node_modules` — into a silent no-op.
        let lockfile_only = lockfile_only
            || (rebuild.is_none()
                && !config.enable_modules_dir
                && !config.enable_global_virtual_store);

        // `--dry-run` resolves but never materializes, so it borrows the
        // lockfile-only plumbing (skip node_modules / `.modules.yaml` /
        // workspace-state) while additionally skipping the lockfile write.
        let resolve_only = lockfile_only || dry_run;

        if config.frozen_store && config.force {
            return Err(InstallError::ConfigConflictFrozenStoreWithForce);
        }

        if config.virtual_store_only
            && !config.enable_modules_dir
            && !config.enable_global_virtual_store
        {
            return Err(InstallError::ConfigConflictVirtualStoreOnlyWithNoModulesDir);
        }

        // Resolve the effective `preferFrozenLockfile` for the
        // dispatch: a per-invocation CLI flag wins over
        // `config.prefer_frozen_lockfile`.
        let prefer_frozen_lockfile =
            prefer_frozen_lockfile.unwrap_or(config.prefer_frozen_lockfile);

        // Collect once so the same set drives both the install dispatch
        // and the `included` field of `.modules.yaml` written below.
        // This is the same set the dependency-graph walker observes.
        let dependency_groups: Vec<DependencyGroup> = dependency_groups.into_iter().collect();
        let included = IncludedDependencies {
            dependencies: dependency_groups.contains(&DependencyGroup::Prod),
            dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
            optional_dependencies: dependency_groups.contains(&DependencyGroup::Optional),
        };

        // Project root for the [bunyan]-envelope `prefix`. This is
        // emitted as `lockfileDir`, the directory containing
        // `pnpm-lock.yaml`. With workspace support that equals the
        // workspace root — pacquet finds it via [`find_workspace_dir`].
        // Falls back to the manifest's parent dir when no
        // `pnpm-workspace.yaml` exists in any ancestor (the
        // single-project case). Closes pnpm/pacquet#357.
        //
        // [bunyan]: <https://github.com/trentm/node-bunyan>
        let manifest_dir = manifest.path().parent().expect("manifest path always has a parent dir");
        let workspace_dir_opt = configured_or_discovered_workspace_dir(config, manifest_dir)
            .map_err(InstallError::FindWorkspaceDir)?;
        // Dedicated per-project lockfiles (`sharedWorkspaceLockfile:
        // false`) anchor everything `workspace_root` names — the wanted
        // lockfile, importer ids, reporter prefixes, the workspace-state
        // file — at the active project, mirroring pnpm's `lockfileDir =
        // sharedWorkspaceLockfile ? workspaceDir : projectDir`. Catalogs
        // and workspace packages still come from the real workspace dir
        // (`workspace_dir_opt`).
        let workspace_root = if config.shared_workspace_lockfile {
            workspace_dir_opt.clone().unwrap_or_else(|| manifest_dir.to_path_buf())
        } else {
            manifest_dir.to_path_buf()
        };

        // Read `pnpm-workspace.yaml` for the catalog sections. Only
        // consulted when a workspace manifest exists — single-project
        // installs have no `catalog:` to honor.
        let workspace_manifest = match workspace_dir_opt.as_deref() {
            Some(dir) => pacquet_workspace::read_workspace_manifest(dir)
                .map_err(InstallError::ReadWorkspaceManifest)?,
            None => None,
        };
        // Prefer a caller-supplied in-memory catalogs set
        // (`catalogs_override`, e.g. `pacquet update --latest --no-save`
        // resolving a bumped `catalog:` entry that is not written to disk),
        // then catalogs an `updateConfig` pnpmfile hook produced
        // (`config.catalogs`, the complete set after the hook pass), and
        // finally the raw workspace-manifest read. `None` at every layer
        // falls back to the manifest, mirroring pnpm's post-`updateConfig`
        // `config.catalogs`.
        let catalogs = match catalogs_override.or_else(|| config.catalogs.clone()) {
            Some(catalogs) => catalogs,
            None => get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
                .map_err(InstallError::InvalidCatalogsConfiguration)?,
        };
        // Use `to_string_lossy` rather than `to_str().expect(...)` so a
        // valid filesystem path with non-UTF-8 bytes (possible on Unix)
        // doesn't panic the installer. `prefix` is used only for
        // reporter envelopes, so a lossy conversion is acceptable —
        // the rest of the install path uses the same pattern for
        // paths threaded into log events.
        let prefix = workspace_root.to_string_lossy().into_owned();

        // Walk every workspace project's `package.json` once. The
        // resulting `Vec` feeds both the up-to-date short-circuit
        // below and the fresh-install path's `workspace:`-spec lookup
        // / per-importer manifest list further down. `None` when no
        // `pnpm-workspace.yaml` exists in or above `workspace_root` —
        // single-project installs only have the root manifest, which
        // the short-circuit and the install paths both reach via
        // `manifest` directly.
        //
        // An embedder that supplies its importers in memory
        // (`workspace_projects_override`) bypasses the on-disk walk
        // entirely; the override's `Vec` is used verbatim.
        let workspace_projects_are_overridden = workspace_projects_override.is_some();
        let loaded_workspace_projects = match (selection.as_ref(), workspace_projects_override) {
            (Some(_), _) => None,
            (None, Some(projects)) => Some(projects),
            (None, None) => load_workspace_projects(
                workspace_dir_opt.as_deref().unwrap_or(&workspace_root),
                workspace_manifest.as_ref(),
            )
            .map_err(InstallError::FindWorkspaceProjects)?,
        };
        let workspace_projects = selection.as_ref().map_or_else(
            || loaded_workspace_projects.as_deref(),
            |selection| Some(selection.all_projects),
        );

        // Optimistic repeat-install short-circuit. When nothing has
        // changed since the previous successful install (settings,
        // workspace structure, manifest mtimes), skip the entire
        // install pipeline and emit pnpm's "Already up to date" log.
        // The fast path runs before any of the install setup (no
        // lockfile reads, no verifier fan-out, no `getContext`).
        //
        // Disabled when `--frozen-lockfile` is requested: an explicit
        // headless install should always go through the dispatch so a
        // `NoLockfile` or `OutdatedLockfile` error still fires when
        // the lockfile is missing or stale.
        let manifest_is_root_importer = root_manifest_as_workspace_root
            || workspace_projects_are_overridden
            || !config.shared_workspace_lockfile;
        let project_manifests = match selection.as_ref() {
            Some(selection) => build_selected_project_manifests_list(
                manifest,
                selection.all_projects,
                selection.active_manifest_is_standin,
            ),
            None if manifest_is_root_importer => build_root_importer_project_manifests_list(
                &workspace_root,
                manifest,
                // Dedicated per-project lockfiles record a single "."
                // importer per project; sibling projects only feed the
                // `workspace:` resolver, never the importer list.
                config.shared_workspace_lockfile.then_some(workspace_projects).flatten(),
            ),
            None => build_project_manifests_list(&workspace_root, manifest, workspace_projects),
        };
        let manifest_freshness_inputs = match selection.as_ref() {
            Some(selection) => selected_manifest_freshness_inputs(
                &workspace_root,
                &project_manifests,
                selection.selected_dirs,
            ),
            None => project_manifests
                .iter()
                .map(|(project_dir, manifest)| {
                    (
                        pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir),
                        *manifest,
                    )
                })
                .collect(),
        };
        let selected_importer_ids = selection.as_ref().map(|selection| {
            selection
                .selected_dirs
                .iter()
                .map(|project_dir| {
                    pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir)
                })
                .collect::<HashSet<_>>()
        });
        let real_importer_ids = project_manifests
            .iter()
            .map(|(project_dir, _)| {
                pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir)
            })
            .collect::<HashSet<_>>();
        let filtered_install = selected_importer_ids
            .as_ref()
            .is_some_and(|selected_importer_ids| selected_importer_ids != &real_importer_ids);
        let requested_importer_ids = if filtered_install { selected_importer_ids } else { None };
        // Only a full `pacquet install` may short-circuit. `add` and
        // `remove` mutate the manifest in memory and persist it after
        // this run returns, so the on-disk mtimes the check reads still
        // describe the pre-mutation project — without this gate a fresh
        // workspace state would read as "nothing changed → already up
        // to date" and the mutation would never be resolved or
        // materialized. `pacquet update` is
        // excluded through its seed policy: a compatible bump leaves
        // the manifest byte-identical, which the check would likewise
        // read as up to date and skip the registry re-resolution.
        let optimistic_decision = is_full_install
            && matches!(update_seed_policy, UpdateSeedPolicy::KeepAll)
            && !filtered_install
            && !frozen_lockfile
            && !config.force
            && !disable_optimistic_repeat_install
            && check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
                workspace_root: &workspace_root,
                config,
                node_linker,
                included,
                supported_architectures: supported_architectures.as_ref(),
                project_manifests: &project_manifests,
                is_workspace_install: workspace_manifest.is_some(),
                lockfile,
                catalogs: &catalogs,
            }) == OptimisticRepeatInstallDecision::UpToDate;
        if optimistic_decision {
            // Keep `strictDepBuilds` enforced across reruns: an install
            // that already recorded unapproved ignored builds must keep
            // failing until they are approved, not exit 0 via the fast
            // path. An `allowBuilds` change that newly permits one is
            // already caught by `settings_match` (the policy is part of
            // the workspace state), which reports drift and skips this
            // branch, so the full install runs and rebuilds it.
            //
            // A corrupt / unreadable `.modules.yaml` can't prove there are
            // no recorded ignored builds, so under strict mode fall through
            // to the full install rather than short-circuiting on a
            // swallowed read error.
            let marker_safe = if gvs_build_markers_may_require_recovery(config) {
                match lockfile.get() {
                    Ok(Some(wanted)) => !gvs_build_marker_present(wanted, config),
                    Ok(None) => true,
                    Err(_) => false,
                }
            } else {
                true
            };
            let strict_builds_safe = if config.strict_dep_builds {
                match pacquet_modules_yaml::read_modules_layout::<Host>(&config.modules_dir) {
                    Ok(Some(modules)) => match unapproved_recorded_ignored_builds(&modules, config)
                    {
                        Ok(Some(package_names)) => {
                            return Err(InstallError::IgnoredBuilds { package_names });
                        }
                        Ok(None) => true,
                        // Unreadable state or a malformed `allowBuilds`:
                        // can't trust the fast path, run the full install.
                        Err(_) => false,
                    },
                    Ok(None) => true,
                    Err(_) => false,
                }
            } else {
                true
            };
            if marker_safe && strict_builds_safe {
                Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                    level: LogLevel::Info,
                    message: "Already up to date".to_string(),
                    prefix: prefix.clone(),
                }));
                Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));
                return Ok(());
            }
        }

        // Past the repeat-install fast path every install flavor needs
        // the wanted lockfile's contents; force the deferred load here.
        let lockfile = lockfile.get().map_err(InstallError::LoadWantedLockfile)?;

        // Register the project against the shared store for prune
        // tracking, once per install at the workspace root. Register
        // the workspace root once, not per importer — store prune walks
        // the workspace's `node_modules/.pnpm/` to find every installed
        // package, so one registry entry per workspace is enough.
        //
        // Gated on `enable_global_virtual_store` because pacquet wires
        // the prune-by-registry path only under GVS for now; pnpm
        // registers unconditionally, so once the non-GVS prune path
        // lands the gate should be dropped. Best-effort: a registry
        // write failure shouldn't fail the install. Surface as
        // `tracing::warn!` so the failure is diagnosable but the
        // install carries on.
        if config.enable_global_virtual_store {
            // Create the store root before calling `register_project` so
            // its `path_contains` guard can canonicalize the path
            // instead of falling through to a literal comparison that
            // wrongly matches against `<workspace>/../pacquet-store/v11`-
            // shaped relative store paths (resolved-on-disk: outside the
            // workspace; lexical: starts with the workspace prefix).
            if let Err(error) =
                std::fs::create_dir_all(pacquet_store_dir::StoreDir::root(&config.store_dir))
            {
                tracing::warn!(
                    target: "pacquet::install",
                    ?error,
                    "Failed to ensure store root exists before project registry write; install continues",
                );
            }
            if let Err(error) =
                pacquet_store_dir::register_project(&config.store_dir, &workspace_root)
            {
                tracing::warn!(
                    target: "pacquet::install",
                    ?error,
                    "Failed to register workspace root in the store project registry; install continues",
                );
            }
        }

        // `pnpm:package-manifest initial` carries the on-disk
        // `package.json` body for this importer. Fires before
        // `pnpm:context` so consumers that key off manifest contents
        // have it ready when the install header renders.
        if emit_initial_manifest {
            emit_initial_package_manifest::<Reporter>(manifest);
        }

        // Load the *current* lockfile that records what the previous
        // install actually materialized in `<virtual_store_dir>/lock.yaml`.
        // The frozen-lockfile path diffs each wanted snapshot against
        // this on a per-`PackageKey` basis to decide whether the
        // already-installed slot is still usable. `Ok(None)` on a
        // first install (the file doesn't exist yet). A corrupted /
        // version-incompatible file is disposable state: pnpm warns and
        // continues with an empty current lockfile because the wanted
        // lockfile and filesystem remain authoritative.
        let current_lockfile =
            match Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir) {
                Ok(lockfile) => lockfile,
                Err(error) => {
                    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                        level: LogLevel::Warn,
                        message: format!(
                            "Ignoring broken lockfile at {}: {error}",
                            config.virtual_store_dir.display(),
                        ),
                        prefix: prefix.clone(),
                    }));
                    None
                }
            };

        // Synthesize the wanted lockfile from `<virtual_store_dir>/lock.yaml`
        // when `pnpm-lock.yaml` is absent and the materialized snapshot still
        // satisfies the manifest. The install then skips resolution and
        // regenerates `pnpm-lock.yaml` from the synthesized object.
        let synthesized_lockfile: Option<Lockfile> =
            if lockfile.is_none() && !frozen_lockfile && prefer_frozen_lockfile {
                current_lockfile.as_ref().and_then(|current| {
                    check_lockfile_freshness(
                        current,
                        &manifest_freshness_inputs,
                        config,
                        &catalogs,
                        ignore_manifest_check,
                        true,
                    )
                    .ok()
                    .map(|()| current.clone())
                })
            } else {
                None
            };
        let lockfile_synthesized_from_current = synthesized_lockfile.is_some();
        // The dry-run diff baseline is the actual on-disk `pnpm-lock.yaml`
        // (`None` when it is absent), captured before the synthesized-from-
        // current fallback below. Diffing against the synthesized lockfile
        // would hide the change of a real install creating `pnpm-lock.yaml`.
        let existing_wanted_lockfile = lockfile;
        let lockfile = lockfile.or(synthesized_lockfile.as_ref());

        // One per-install packument cache shared with both the
        // lockfile-verifier (below) and the resolver in
        // `install_with_fresh_lockfile` (further down). The
        // single instance lets a name the resolver fetched during this
        // install short-circuit the verifier's own fetch chain, and
        // vice versa.
        let meta_cache = Arc::new(InMemoryPackageMetaCache::default());
        // Resolution verifiers re-apply `minimumReleaseAge` /
        // `trustPolicy='no-downgrade'` (plus the tarball-URL anti-tamper
        // check) to every entry in the loaded `pnpm-lock.yaml`. They are
        // built here — cheap, no I/O — but the verification fan-out itself
        // is dispatched per path below: on the frozen materialization path
        // it runs concurrently with the fetch (see [`InstallFrozenLockfile`])
        // so the per-entry registry round trips overlap the download;
        // every other path (fresh resolve, the lockfile-only / up-to-date
        // short-circuits) verifies eagerly via [`verify_lockfile_eagerly`]
        // before it proceeds. `trust_lockfile` (the OR of yaml's
        // `trustLockfile` and the `--trust-lockfile` CLI flag, resolved in
        // [`crate::cli_args::install::InstallArgs::run`]; the opt-out for
        // environments that treat the on-disk lockfile as
        // already-trusted) or no active resolution policy leaves the list
        // empty, making every gate a no-op — fresh local resolution is
        // already filtered by the resolver's own per-version gate
        // (`minimumReleaseAge` via `ResolveResult::policy_violation`,
        // `trustPolicy='no-downgrade'` via the npm resolver's
        // `fail_if_trust_downgraded_for_pick`). The list is built whenever
        // a policy could apply, independent of whether a lockfile is loaded, so the
        // fresh-resolve path can record the freshly written lockfile as
        // already-verified (see `record_lockfile_verified` below).
        let resolution_verifiers = if trust_lockfile {
            Vec::new()
        } else {
            build_resolution_verifiers(
                config,
                Arc::clone(&http_client_arc),
                Some(Arc::clone(&meta_cache)
                    as Arc<dyn pacquet_resolving_npm_resolver::PackageMetaCache>),
                auth_override.clone(),
                None,
            )
            .map_err(InstallError::BuildVerifiers)?
        };
        let derived_lockfile_path = lockfile.map(|_| {
            lockfile_path
                .map_or_else(|| workspace_root.join(Lockfile::FILE_NAME), Path::to_path_buf)
        });

        // `pnpm:context` carries the directories pnpm's reporter prints
        // in the install header. `currentLockfileExists` is `true` once
        // a previous install has written `<virtual_store_dir>/lock.yaml`.
        Reporter::emit(&LogEvent::Context(ContextLog {
            level: LogLevel::Debug,
            current_lockfile_exists: current_lockfile.is_some(),
            store_dir: config.store_dir.display().to_string(),
            virtual_store_dir: config.effective_virtual_store_dir().to_string_lossy().into_owned(),
        }));

        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: prefix.clone(),
            stage: Stage::ImportingStarted,
        }));

        // Install-scoped dedupe state for `pnpm:package-import-method`.
        // Threaded down to `link_file::log_method_once` so each install
        // emits the channel afresh — a per-importer capture rather than
        // a process-static.
        let logged_methods = AtomicU8::new(0);

        tracing::info!(target: "pacquet::install", "Start all");

        // Dispatch priority, following the CLI + `preferFrozenLockfile`
        // semantics:
        //
        // 1. `--frozen-lockfile` flag → frozen path. Lockfile must exist
        //    and the freshness check (settings + per-importer specifier
        //    match) must pass, otherwise fail.
        //
        // 2. No flag, lockfile present, `prefer_frozen_lockfile == true`,
        //    and the freshness check passes → frozen path (same code as
        //    state 1). The `preferFrozenLockfile` fast path: when the
        //    lockfile matches the manifest, the install silently goes
        //    headless instead of re-resolving against the registry.
        //
        // 3. No flag, lockfile present, but either `prefer_frozen_lockfile`
        //    is off or the freshness check fails → fresh-resolve path,
        //    seeded from the existing lockfile so unrelated entries keep
        //    their pins (the `update: false` resolver mode).
        //
        // 4. No lockfile → fresh-resolve path with no seed, writes a
        //    brand-new `pnpm-lock.yaml`.
        //
        // The third tuple element is `hoisted_locations`: the per-depPath
        // list of lockfile-relative directories the hoisted linker placed
        // each package at. Empty under the isolated linker (and under the
        // no-lockfile path); non-empty only when the frozen-lockfile
        // install runs with `nodeLinker: hoisted`. Threaded into
        // `build_modules_manifest` so the field is persisted into
        // `.modules.yaml.hoisted_locations` for the next install and for
        // the rebuild path (which throws `MISSING_HOISTED_LOCATIONS` when
        // this field is gone).

        if update_checksums && frozen_lockfile {
            return Err(InstallError::FrozenLockfileWithUpdateChecksums);
        }

        // Compute the dispatch decision once. `take_frozen_path` is true
        // for both state 1 (--frozen-lockfile) and state 2 (auto-frozen
        // via prefer-frozen-lockfile). The freshness check fires for both
        // — fatal for state 1, fall-through for state 2.
        //
        // `--dry-run` always takes the fresh-resolve path: it must compute
        // the would-be lockfile to diff against the existing one, and the
        // frozen freshness gate would otherwise abort on a stale lockfile
        // instead of reporting the change.
        let take_frozen_path = if dry_run {
            false
        } else if frozen_lockfile {
            let Some(lockfile) = lockfile else {
                return Err(InstallError::NoLockfile);
            };
            // Run the freshness gates; on failure surface a fatal
            // InstallError via `FreshnessCheckError`'s `From` impl.
            // The check is run for its side effect (the typed
            // outcome) — the borrowed lockfile / manifests are consumed
            // again inside the frozen branch below.
            check_lockfile_freshness(
                lockfile,
                &manifest_freshness_inputs,
                config,
                &catalogs,
                ignore_manifest_check,
                false,
            )
            .map_err(InstallError::from)?;
            true
        } else if update_checksums {
            false
        } else if let Some(lockfile) = lockfile {
            // Auto-frozen via `preferFrozenLockfile`. Skip when the
            // user opted out (`--no-prefer-frozen-lockfile` /
            // `preferFrozenLockfile: false`); otherwise consult the
            // freshness gate. A `Stale` / `NoImporter` outcome routes
            // to the fresh-resolve path; a malformed
            // `pnpm.overrides` is a user-config error that surfaces
            // regardless of dispatch.
            if prefer_frozen_lockfile {
                match check_lockfile_freshness(
                    lockfile,
                    &manifest_freshness_inputs,
                    config,
                    &catalogs,
                    ignore_manifest_check,
                    true,
                ) {
                    // Even an up-to-date lockfile may not go frozen: a
                    // custom resolver's `shouldRefreshResolution` can
                    // force the fresh-resolve path. The hook's verdict
                    // blocks the frozen install. A lockfile
                    // synthesized from the current snapshot skips the
                    // check (it only gates on a non-empty wanted
                    // lockfile). A throwing hook aborts the install.
                    Ok(()) => {
                        lockfile_synthesized_from_current
                            || !crate::check_custom_resolver_force_resolve::force_resolve_from_pnpmfile(
                                lockfile,
                                &workspace_root,
                            )
                            .await
                            .map_err(InstallError::CustomResolverForceResolve)?
                    }
                    Err(FreshnessCheckError::Stale(_) | FreshnessCheckError::NoImporter { .. }) => {
                        false
                    }
                    Err(
                        error @ (FreshnessCheckError::InvalidOverrides(_)
                        | FreshnessCheckError::CalcPatchHashes(_)),
                    ) => {
                        return Err(error.into());
                    }
                }
            } else {
                false
            }
        } else {
            false
        };

        // `--lockfile-only`: resolve and (re)write `pnpm-lock.yaml`, then
        // stop — never materialize `node_modules`, `.modules.yaml`, the
        // current lockfile, or the workspace-state file. The
        // `lockfileOnly` short-circuits: the frozen / up-to-date path
        // writes the wanted lockfile and returns, and the fresh-resolve
        // path skips `linkPackages`.
        if lockfile_only && take_frozen_path {
            // Frozen (`--frozen-lockfile`) or auto-frozen
            // (`preferFrozenLockfile`) + `--lockfile-only`: the freshness
            // gate folded into `take_frozen_path` already validated the
            // on-disk lockfile (a stale one surfaced `OutdatedLockfile`).
            // Re-persist it so a brand-new project still lands a file, then
            // return without touching `node_modules`.
            let lockfile = lockfile.expect("frozen dispatch verified lockfile is present");
            // This path materializes nothing, so there's no fetch to overlap;
            // verify eagerly to keep the gate before the early return.
            if let Some(lockfile_verification_override) = lockfile_verification_override {
                lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
            } else {
                verify_lockfile_eagerly::<Reporter>(
                    lockfile,
                    &resolution_verifiers,
                    derived_lockfile_path.as_deref(),
                    &config.cache_dir,
                )
                .await?;
            }
            if config.lockfile {
                lockfile
                    .save_to_path(&workspace_root.join(Lockfile::FILE_NAME))
                    .map_err(InstallError::SaveWantedLockfile)?;
            }
            Reporter::emit(&LogEvent::Stage(StageLog {
                level: LogLevel::Debug,
                prefix: prefix.clone(),
                stage: Stage::ImportingDone,
            }));
            Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));
            return Ok(());
        }

        // No-op short-circuit. When the frozen-lockfile dispatch is
        // eligible, the on-disk `.modules.yaml` agrees with the current
        // config, and `<virtual_store_dir>/lock.yaml` is byte-equal to
        // the wanted lockfile, nothing needs to be materialized — the
        // last install already produced exactly this `node_modules`.
        // Emit the up-to-date log, refresh the workspace-state
        // timestamp so `pnpm run`'s `verifyDepsBeforeRun` doesn't fire
        // spuriously, then exit.
        // Parse `.modules.yaml` once and share it across the consistency,
        // newly-allowed, and unapproved-ignored checks below.
        let modules_manifest_res = if !resolve_only || take_frozen_path {
            pacquet_modules_yaml::read_modules_layout::<Host>(&config.modules_dir)
        } else {
            Ok(None)
        };
        let read_failed = modules_manifest_res.is_err();
        if let Err(err) = &modules_manifest_res {
            tracing::warn!(
                target: "pacquet::install",
                ?err,
                "failed to read .modules.yaml; treating as an inconsistent node_modules directory",
            );
        }
        let old_modules = modules_manifest_res.ok().flatten();
        let modules_manifest = old_modules.as_ref();
        // A filtered install rewrites `.modules.yaml` from the selected
        // projects' state merged over the previous file's, so losing the
        // previous contents would drop every unselected project's entries.
        // An unreadable *layout* is already handled as an inconsistent
        // `node_modules` — the purge rebuilds everything and the merge is
        // skipped — but a file whose layout parses while some later field
        // does not would otherwise merge against `None` and silently prune
        // those entries, so that case fails instead.
        let previous_modules_metadata = if !resolve_only && !read_failed {
            match pacquet_modules_yaml::read_modules_manifest::<Host>(&config.modules_dir) {
                Ok(modules) => modules,
                // A filtered install merges the unselected importers'
                // entries out of this file, so it cannot proceed
                // without it; an unfiltered install only loses the
                // orphan hoist-link cleanup.
                Err(error) if filtered_install => return Err(InstallError::ReadModules(error)),
                Err(error) => {
                    tracing::warn!(
                        target: "pacquet::install",
                        ?error,
                        "failed to fully parse .modules.yaml; skipping orphan hoist-link cleanup",
                    );
                    None
                }
            }
        } else {
            None
        };
        let prior_hoisted_dependencies =
            previous_modules_metadata.as_ref().map(|modules| &modules.hoisted_dependencies);
        // On a filtered install the wanted lockfile only covers the
        // selected importers; a snapshot diff against it would misread
        // every unselected importer's packages as orphans.
        let prune_orphans = !filtered_install;

        // The purge keys off *layout* drift only, not `included`: an
        // included (`--prod`<->full) change is handled by relinking, so it
        // must not wipe the user's `node_modules` contents. See
        // [`modules_layout_consistent_with`].
        let is_inconsistent = read_failed
            || match &modules_manifest {
                Some(modules) => !modules_layout_consistent_with(modules, config, node_linker),
                // Treat existence-check errors conservatively as inconsistent.
                None => config
                    .modules_dir
                    .join(pacquet_modules_yaml::MODULES_FILENAME)
                    .try_exists()
                    .unwrap_or(true),
            };

        if !resolve_only && is_inconsistent {
            // A plain install may recreate the drifted modules dir;
            // `add` / `remove` must surface the drift instead
            // (upstream `validateModules` with `forceNewModules =
            // installsOnly`).
            if !installs_only && let Some(modules) = modules_manifest {
                check_modules_settings_diff(modules, config)?;
            }
            // Settings mismatch forces a rewrite of node_modules.
            let (is_safe, target_dir) = if config.modules_dir.exists() {
                match (
                    std::fs::canonicalize(&config.modules_dir),
                    std::fs::canonicalize(&workspace_root),
                ) {
                    (Ok(modules_canon), Ok(root_canon)) => {
                        (modules_canon.starts_with(&root_canon), Some(modules_canon))
                    }
                    _ => (false, None),
                }
            } else {
                (true, None)
            };
            if is_safe {
                if let Some(target) = target_dir {
                    match std::fs::read_dir(&target) {
                        Ok(entries) => {
                            for entry_res in entries {
                                let entry = match entry_res {
                                    Ok(e) => e,
                                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                                        continue;
                                    }
                                    Err(err) => return Err(InstallError::RemoveModulesDir(err)),
                                };
                                let file_name = entry.file_name();
                                let file_name_str = file_name.to_string_lossy();

                                let is_hidden = file_name_str.starts_with('.');
                                let is_pnpm_hidden = file_name_str == ".bin"
                                    || file_name_str == ".modules.yaml"
                                    || config
                                        .virtual_store_dir
                                        .file_name()
                                        .is_some_and(|n| n == file_name_str.as_ref())
                                    || modules_manifest.as_ref().is_some_and(|manifest| {
                                        let mut old_vs =
                                            std::path::PathBuf::from(&manifest.virtual_store_dir);
                                        if old_vs.is_relative() {
                                            old_vs = config.modules_dir.join(old_vs);
                                        }
                                        old_vs.starts_with(&config.modules_dir)
                                            && old_vs
                                                .file_name()
                                                .is_some_and(|n| n == file_name_str.as_ref())
                                    });

                                if is_hidden && !is_pnpm_hidden {
                                    continue;
                                }

                                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                                    #[cfg(windows)]
                                    let is_removed =
                                        pacquet_fs::remove_symlink_dir(&entry.path()).is_ok();
                                    #[cfg(not(windows))]
                                    let is_removed = false;

                                    if !is_removed
                                        && let Err(err) = std::fs::remove_dir_all(entry.path())
                                        && err.kind() != std::io::ErrorKind::NotFound
                                    {
                                        return Err(InstallError::RemoveModulesDir(err));
                                    }
                                } else if let Err(err) = std::fs::remove_file(entry.path())
                                    && err.kind() != std::io::ErrorKind::NotFound
                                {
                                    return Err(InstallError::RemoveModulesDir(err));
                                }
                            }
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                        Err(err) => return Err(InstallError::RemoveModulesDir(err)),
                    }
                }
            } else {
                if filtered_install {
                    return Err(InstallError::UnsafeFilteredModulesDir {
                        modules_dir: config.modules_dir.clone(),
                        workspace_root: workspace_root.clone(),
                    });
                }
                tracing::warn!(
                    ?config.modules_dir,
                    "refusing to remove inconsistent modules directory outside the project root",
                );
            }
        }

        // Remove direct links from dependency groups excluded by this
        // run. Unfiltered installs can use the global `included` value
        // recorded in `.modules.yaml`; filtered installs may retain
        // importers materialized with different group sets, so they
        // conservatively prune every excluded group from only the
        // selected workspace-link closure.
        if !resolve_only
            && !is_inconsistent
            && let Some(modules) = modules_manifest
            && let Some(current) = current_lockfile.as_ref()
            && (filtered_install || modules.included != included)
        {
            let selected_prune_importer_ids = requested_importer_ids.as_ref().map(|requested| {
                crate::materialization_closure(
                    current,
                    &workspace_root,
                    requested,
                    included,
                    &crate::SkippedSnapshots::new(),
                )
                .importer_ids
            });
            let previously_included = if filtered_install {
                IncludedDependencies {
                    dependencies: true,
                    dev_dependencies: true,
                    optional_dependencies: true,
                }
            } else {
                modules.included
            };
            crate::prune_direct_deps_excluded_by_groups(
                current,
                previously_included,
                included,
                &workspace_root,
                config,
                selected_prune_importer_ids.as_ref(),
            )
            .map_err(InstallError::PruneDirectDeps)?;
        }

        let modules_cache_prune_due = modules_manifest.as_ref().is_some_and(|modules| {
            crate::prune_virtual_store::should_prune_virtual_store(
                crate::prune_virtual_store::same_dir(
                    config.effective_virtual_store_dir(),
                    &config.global_virtual_store_dir,
                ),
                Some(modules.pruned_at.as_str()),
                config.modules_cache_max_age,
                SystemTime::now(),
            )
        });

        if take_frozen_path
            && !filtered_install
            // `--force` reinstalls everything, so an up-to-date tree
            // must not short-circuit the materialization.
            && !config.force
            && let Some(wanted_lockfile) = lockfile
            && let Some(current) = current_lockfile.as_ref()
            && wanted_lockfile == current
            && let Some(modules) = modules_manifest.as_ref()
            && modules_consistent_with(modules, config, node_linker, included)
            // A `supportedArchitectures` change alters the skip set
            // without touching the lockfile or `.modules.yaml`, so the
            // unchanged-layout premise doesn't hold and the platform
            // packages must be re-evaluated.
            && crate::optimistic_repeat_install::recorded_supported_architectures_match(
                &workspace_root,
                supported_architectures.as_ref(),
            )
            // An `allowBuilds` change that now permits a previously-ignored
            // build must rebuild it, even though the lockfile and layout are
            // unchanged.
            && !has_newly_allowed_ignored_builds(modules, config)
            // The mirror image: an approval the user has since withdrawn
            // must be re-evaluated, or a strict install would exit 0 on a
            // package it is no longer allowed to build.
            && !has_revoked_allowed_builds(modules, config)
            // A build marker lives in the shared slot, outside every
            // project-state input checked above. Let materialization inspect
            // buildable and patched GVS slots instead of declaring the local
            // tree complete from importer links alone.
            && !gvs_build_marker_present(wanted_lockfile, config)
            // An explicit `pacquet rebuild` always re-runs the build phase,
            // so it never short-circuits here.
            && rebuild.is_none()
            && !modules_cache_prune_due
            && frozen_tree_intact(wanted_lockfile, modules, config, &workspace_root, node_linker)
        {
            // The full frozen path runs the offline structural
            // name gate before any materialization; the up-to-date
            // early return must not skip it (the resolution-verifier
            // fan-out below is policy-gated and can be empty).
            pacquet_lockfile_verification::verify_lockfile_dependency_names(wanted_lockfile)
                .map_err(InstallError::LockfileVerification)?;
            // Nothing to materialize means no fetch to overlap; verify
            // eagerly before the up-to-date early return.
            if let Some(lockfile_verification_override) = lockfile_verification_override {
                lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
            } else {
                verify_lockfile_eagerly::<Reporter>(
                    wanted_lockfile,
                    &resolution_verifiers,
                    derived_lockfile_path.as_deref(),
                    &config.cache_dir,
                )
                .await?;
            }
            // Keep `strictDepBuilds` enforced on the up-to-date path: a
            // rerun after an `ERR_PNPM_IGNORED_BUILDS` failure must not
            // exit 0 just because the lockfile and layout are unchanged.
            // Checked after verification (a tampered lockfile fails first)
            // and before the "up to date" log so the command doesn't
            // claim success.
            // `Err` (malformed `allowBuilds`) is unreachable here — the
            // `has_newly_allowed_ignored_builds` guard above returns `true`
            // on the same `from_config` error and skips this block — so a
            // bad policy is surfaced by the full install instead.
            if config.strict_dep_builds
                && let Ok(Some(package_names)) = unapproved_recorded_ignored_builds(modules, config)
            {
                return Err(InstallError::IgnoredBuilds { package_names });
            }
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Info,
                message: "Lockfile is up to date, resolution step is skipped".to_string(),
                prefix: prefix.clone(),
            }));
            Reporter::emit(&LogEvent::Stage(StageLog {
                level: LogLevel::Debug,
                prefix: prefix.clone(),
                stage: Stage::ImportingDone,
            }));
            if lockfile_synthesized_from_current && config.lockfile {
                wanted_lockfile
                    .save_to_path(&workspace_root.join(Lockfile::FILE_NAME))
                    .map_err(InstallError::SaveWantedLockfile)?;
            }
            update_workspace_state(
                &workspace_root,
                &build_workspace_state(
                    &workspace_root,
                    config,
                    node_linker,
                    included,
                    supported_architectures.as_ref(),
                    &catalogs,
                    &project_manifests,
                    filtered_install,
                ),
            )
            .map_err(InstallError::WriteWorkspaceState)?;
            Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));
            return Ok(());
        }

        // Sorted `name@version` keys whose builds were blocked; assigned
        // by whichever path runs and consumed by the `strictDepBuilds`
        // gate at the tail. Kept out of the tuple below (along with the
        // injected-deps map) to avoid a `clippy::type_complexity`
        // annotation.
        let ignored_builds: Vec<String>;
        // Dep paths whose build `--ignore-scripts` deferred; assigned by
        // whichever path runs and folded into `.modules.yaml`'s
        // `pendingBuilds` at the tail.
        let deferred_builds: Vec<String>;
        // Per-source-project virtual-store copies of injected `file:`
        // deps, for `.modules.yaml`'s `injectedDeps`; assigned by
        // whichever path runs. See [`crate::collect_injected_deps`].
        let injected_deps: BTreeMap<String, Vec<String>>;
        let effective_node_version = config
            .node_version
            .clone()
            .or_else(|| node_version_from_engines_runtime(manifest.value()));
        let (hoisted_dependencies, hoisted_locations, install_skipped, fresh_lockfile): (
            HoistedDependencies,
            BTreeMap<String, Vec<String>>,
            crate::SkippedSnapshots,
            Option<Lockfile>,
        ) = if take_frozen_path {
            let lockfile = lockfile.expect("dispatch verified lockfile is present");
            // pnpm's headless installer announces itself whenever it is
            // entered — also on a cold `node_modules` and on subset
            // (`--filter`) installs — not only when nothing needs to be
            // materialized. `pnpm fetch` gets upstream's
            // ignorePackageManifest wording instead; it is the one
            // caller combining `ignore_manifest_check` with a non-full
            // install, and the flag alone can't identify it because
            // `install --ignore-manifest-check` is a user-facing way to
            // skip the frozen freshness gate on a full install.
            // Upstream's headless entry returns before the announcement
            // for an empty lockfile (`isEmptyLockfile`), and an explicit
            // `pnpm rebuild` is not an install, so both stay silent.
            if rebuild.is_none() && !lockfile.is_empty() {
                let message = if ignore_manifest_check && !is_full_install {
                    "Importing packages to virtual store"
                } else {
                    "Lockfile is up to date, resolution step is skipped"
                };
                Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                    level: LogLevel::Info,
                    message: message.to_string(),
                    prefix: prefix.clone(),
                }));
            }
            let initial_materialization_ids = requested_importer_ids.as_ref().map(|selected| {
                if matches!(node_linker, NodeLinker::Hoisted) {
                    lockfile.importers.keys().cloned().collect()
                } else {
                    selected.clone()
                }
            });
            let empty_skipped = crate::SkippedSnapshots::new();
            let materialization = initial_materialization_ids.as_ref().map(|importer_ids| {
                crate::materialization_closure(
                    lockfile,
                    &workspace_root,
                    importer_ids,
                    included,
                    &empty_skipped,
                )
            });
            let materialization_lockfile =
                materialization.as_ref().map_or(lockfile, |closure| &closure.lockfile);
            let project_anchor_ids = match requested_importer_ids.as_ref() {
                Some(selected) if matches!(node_linker, NodeLinker::Hoisted) => selected.clone(),
                Some(_) => materialization
                    .as_ref()
                    .expect("selected install has a materialization closure")
                    .importer_ids
                    .clone(),
                None => real_importer_ids.clone(),
            };
            let frozen_project_manifests = project_manifests
                .iter()
                .filter(|(project_dir, _)| {
                    let importer_id =
                        pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir);
                    project_anchor_ids.contains(&importer_id)
                })
                .cloned()
                .collect::<Vec<_>>();
            let Lockfile { lockfile_version, importers, packages, snapshots, .. } =
                materialization_lockfile;
            assert_eq!(lockfile_version.major, 9); // compatibility check already happens at serde, but this still helps preventing programmer mistakes.

            let mut frozen_verification_override = lockfile_verification_override;
            if requested_importer_ids.is_some() {
                if let Some(verification_override) = frozen_verification_override.take() {
                    verification_override.await.map_err(map_frozen_lockfile_error)?;
                } else {
                    verify_lockfile_eagerly::<Reporter>(
                        lockfile,
                        &resolution_verifiers,
                        derived_lockfile_path.as_deref(),
                        &config.cache_dir,
                    )
                    .await?;
                }
            }
            let frozen_resolution_verifiers = if requested_importer_ids.is_some() {
                &[][..]
            } else {
                resolution_verifiers.as_slice()
            };

            let frozen_result = InstallFrozenLockfile {
                http_client,
                config,
                importers,
                packages: packages.as_ref(),
                snapshots: snapshots.as_ref(),
                lockfile: materialization_lockfile,
                resolution_verifiers: frozen_resolution_verifiers,
                lockfile_verification_override: frozen_verification_override,
                lockfile_path: derived_lockfile_path.as_deref(),
                current_lockfile: current_lockfile.as_ref(),
                // `--force` relinks every package, so the per-snapshot
                // "unchanged since the previous install" skip must not
                // see the current lockfile — pnpm's
                // `lockfileToDepGraph(..., opts.force ? null :
                // currentLockfile)`. `current_lockfile` itself stays:
                // pnpm's prune runs on the real current lockfile even
                // under force.
                current_snapshots: (!config.force)
                    .then_some(current_lockfile.as_ref())
                    .flatten()
                    .and_then(|lockfile| lockfile.snapshots.as_ref()),
                current_packages: (!config.force)
                    .then_some(current_lockfile.as_ref())
                    .flatten()
                    .and_then(|lockfile| lockfile.packages.as_ref()),
                dependency_groups,
                project_manifests: &frozen_project_manifests,
                package_map_project_manifests: &project_manifests,
                logged_methods: &logged_methods,
                workspace_root: &workspace_root,
                requester: &prefix,
                supported_architectures: supported_architectures.as_ref(),
                skip_runtimes,
                node_version: effective_node_version.clone(),
                node_linker,
                tarball_mem_cache: Some(&tarball_mem_cache),
                seed_skipped: modules_manifest.map(|manifest| manifest.skipped.clone()),
                rebuild: rebuild.as_ref(),
                prior_hoisted_dependencies,
                prune_orphans,
            }
            .run::<Reporter>()
            .await
            // Surface a verification failure as the same top-level
            // `LockfileVerification` variant the eager paths use, rather
            // than nesting it under `FrozenLockfile` — the concurrent gate
            // is the same gate, just run alongside the fetch.
            .map_err(map_frozen_lockfile_error)?;

            ignored_builds = frozen_result.ignored_builds;
            deferred_builds = frozen_result.deferred_builds;
            injected_deps = frozen_result.injected_deps;
            (
                frozen_result.hoisted_dependencies,
                frozen_result.hoisted_locations,
                frozen_result.skipped,
                None,
            )
        } else {
            // Re-verify the existing lockfile before the fresh resolve,
            // matching the pre-resolution gate: a committed lockfile that
            // bypassed the policy locally is caught here even though the
            // resolver re-resolves from it. No-op when there's no lockfile
            // (state 4) or verification is disabled. The fresh path's own
            // resolution is the slow part, so this stays a blocking gate.
            if let Some(lockfile_verification_override) = lockfile_verification_override {
                lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
            } else if let Some(loaded_lockfile) = lockfile {
                verify_lockfile_eagerly::<Reporter>(
                    loaded_lockfile,
                    &resolution_verifiers,
                    derived_lockfile_path.as_deref(),
                    &config.cache_dir,
                )
                .await?;
            }

            // The fresh-lockfile path has no installability check
            // (no `packages:` metadata to evaluate constraints
            // against), so its skip set is empty by construction.
            // Walk every workspace project once: the returned `Vec`
            // feeds both the `workspace:`-spec lookup the npm resolver
            // consults *and* the per-importer manifest list the
            // resolver iterates over. `None` workspace projects when
            // the install isn't inside a `pnpm-workspace.yaml`
            // workspace (no workspace root was found) — the resolver
            // then errors out on any `workspace:` spec rather than
            // silently skipping to a registry lookup.
            //
            // Reuses the `workspace_projects` walk done at the top of
            // `Install::run` for the optimistic-repeat-install check
            // so we don't pay the workspace scan twice on a
            // fresh-install fall-through.
            let workspace_packages = build_workspace_packages_map(workspace_projects);
            // Build the per-importer manifest list. The root importer
            // (`"."`) always reuses the in-memory `Install.manifest`
            // — `pacquet add` mutates that value before calling install,
            // so re-reading from disk would walk the pre-add shape and
            // miss the freshly-added dep. Sibling importers come from
            // the `find_workspace_projects` walk, which read them off
            // disk for `workspace_packages` already.
            let importer_manifests: BTreeMap<String, &PackageManifest> = project_manifests
                .iter()
                .map(|(project_dir, manifest)| {
                    (
                        pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir),
                        *manifest,
                    )
                })
                .collect();
            let fresh_result = InstallWithFreshLockfile {
                tarball_mem_cache,
                resolved_packages,
                http_client,
                http_client_arc: Arc::clone(&http_client_arc),
                config,
                importer_manifests,
                dependency_groups,
                logged_methods: &logged_methods,
                requester: &prefix,
                catalogs: catalogs.clone(),
                lockfile_dir: &workspace_root,
                workspace_packages,
                update_checksums,
                meta_cache: Arc::clone(&meta_cache),
                // States 3 and 4 of the dispatch share this branch.
                // State 3 (lockfile present but stale or
                // `preferFrozenLockfile: false`) passes the existing
                // lockfile so the resolver seeds
                // `getPreferredVersionsFromLockfileAndManifests` with
                // already-pinned `(name, version)` pairs — unrelated
                // entries keep their pins on rewrite (the `update: false`
                // mode). State 4 (no lockfile) passes `None`.
                wanted_lockfile: lockfile,
                node_version: effective_node_version,
                node_linker,
                supported_architectures: supported_architectures.as_ref(),
                lockfile_only: resolve_only,
                skip_runtimes,
                dry_run,
                can_prompt,
                is_full_install,
                update_seed_policy,
                auth_override,
                resolution_observer,
                peer_issues_sink: peer_issues_sink.clone(),
                pnpmfile_hook_override,
                real_importer_ids: requested_importer_ids.as_ref().map(|_| &real_importer_ids),
                selected_importer_ids: requested_importer_ids.as_ref(),
                current_lockfile: current_lockfile.as_ref(),
                prior_hoisted_dependencies,
                prune_orphans,
            }
            .run::<Reporter>()
            .await
            .map_err(InstallError::WithFreshLockfile)?;

            if fresh_result.can_record_lockfile_verification
                && let Some(lockfile) = fresh_result.wanted_lockfile.as_ref()
            {
                // Record under the same path the verification gates key
                // their cache on, so the next install's stat shortcut hits.
                let lockfile_path = derived_lockfile_path
                    .clone()
                    .unwrap_or_else(|| workspace_root.join(Lockfile::FILE_NAME));
                record_lockfile_verified(
                    Some(&config.cache_dir),
                    &lockfile_path,
                    lockfile,
                    &resolution_verifiers,
                );
            }

            ignored_builds = fresh_result.ignored_builds;
            deferred_builds = fresh_result.deferred_builds;
            injected_deps = fresh_result.injected_deps;
            (
                fresh_result.hoisted_dependencies,
                fresh_result.hoisted_locations,
                fresh_result.skipped,
                fresh_result.wanted_lockfile,
            )
        };

        tracing::info!(target: "pacquet::install", "Complete all");

        // Fresh-resolve `--lockfile-only` already wrote `pnpm-lock.yaml` and
        // emitted `importing_done` inside `InstallWithFreshLockfile::run`.
        // Skip `.modules.yaml`, the current lockfile, and the
        // workspace-state file: there is no `node_modules` to describe, and
        // writing the workspace-state file would make the next install's
        // up-to-date check believe materialization happened.
        if resolve_only {
            // `--dry-run` resolved a fresh lockfile but wrote nothing. Diff
            // it against the existing on-disk lockfile and print a report,
            // then exit 0 — npm-style preview semantics. A sink-driven dry
            // run (napi `getPeerDependencyIssues`) is a programmatic query,
            // not a preview — no report.
            if dry_run && peer_issues_sink_is_none {
                use std::io::Write as _;
                let report =
                    crate::dry_run::render_dry_run_report(&crate::dry_run::diff_lockfiles(
                        existing_wanted_lockfile,
                        fresh_lockfile.as_ref(),
                    ));
                let mut stdout = std::io::stdout();
                let _ = writeln!(stdout, "{report}");
                let _ = stdout.flush();
            }
            Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));
            return Ok(());
        }

        let materialized_wanted_lockfile = fresh_lockfile.as_ref().or(lockfile);
        let selected_current_lockfile = materialized_wanted_lockfile.and_then(|wanted| {
            requested_importer_ids.as_ref().map(|requested| {
                crate::materialization_closure(
                    wanted,
                    &workspace_root,
                    requested,
                    included,
                    &install_skipped,
                )
                .lockfile
            })
        });
        let materialized_current_lockfile = materialized_wanted_lockfile.map(|wanted| {
            if requested_importer_ids.is_some() && matches!(node_linker, NodeLinker::Hoisted) {
                crate::filter_lockfile_for_current(wanted, included, &install_skipped)
            } else if let Some(requested_importer_ids) = requested_importer_ids.as_ref() {
                crate::merge_filtered_current_lockfile(
                    (!is_inconsistent).then_some(current_lockfile.as_ref()).flatten(),
                    wanted,
                    requested_importer_ids,
                    included,
                    &install_skipped,
                    &workspace_root,
                )
            } else {
                crate::filter_lockfile_for_current(wanted, included, &install_skipped)
            }
        });
        let project_anchor_importer_ids = match requested_importer_ids.as_ref() {
            Some(requested) if matches!(node_linker, NodeLinker::Hoisted) => requested.clone(),
            Some(requested) => materialized_wanted_lockfile.map_or_else(
                || requested.clone(),
                |wanted| {
                    crate::materialization_closure(
                        wanted,
                        &workspace_root,
                        requested,
                        included,
                        &install_skipped,
                    )
                    .importer_ids
                },
            ),
            None => real_importer_ids.clone(),
        };
        let materialized_project_manifests = project_manifests
            .iter()
            .filter(|(project_dir, _)| {
                let importer_id =
                    pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir);
                project_anchor_importer_ids.contains(&importer_id)
            })
            .cloned()
            .collect::<Vec<_>>();

        if filtered_install
            && !matches!(node_linker, NodeLinker::Hoisted)
            && crate::should_write_package_map(config, node_linker)
            && let Some(current) = materialized_current_lockfile.as_ref()
        {
            let runtime_major =
                crate::install_frozen_lockfile::find_runtime_node_major(current.snapshots.as_ref());
            let configured_major = config
                .node_version
                .as_deref()
                .and_then(crate::install_frozen_lockfile::parse_major_from_version);
            let engine_name = match runtime_major.or(configured_major) {
                Some(major) => Some(pacquet_graph_hasher::engine_name(major, None, None)),
                None if config.enable_global_virtual_store => tokio::task::spawn_blocking(|| {
                    pacquet_graph_hasher::detect_node_major()
                        .map(|major| pacquet_graph_hasher::engine_name(major, None, None))
                })
                .await
                .ok()
                .flatten(),
                None => None,
            };
            let allow_build_policy = crate::AllowBuildPolicy::from_config(config)
                .expect("allow-build policy was validated by the install path");
            let layout = crate::VirtualStoreLayout::new(
                config,
                engine_name.as_deref(),
                current.snapshots.as_ref(),
                current.packages.as_ref(),
                Some(&allow_build_policy),
            );
            crate::package_map::write_package_map(
                current,
                &crate::package_map::PackageMapOptions {
                    lockfile_dir: &workspace_root,
                    modules_dir: &config.modules_dir,
                    package_map_type: config.node_package_map_type,
                    layout: &layout,
                    project_manifests: &project_manifests,
                },
            )
            .map_err(InstallError::WritePackageMap)?;
        }

        // Materialize `link:` direct deps straight from the in-memory
        // project manifests. `excludeLinksFromLockfile` keeps them out
        // of the lockfile importers, so the lockfile-driven symlink
        // passes inside the frozen/fresh paths never see them; pnpm
        // v11's `linkDirectDeps` linked them from the projects
        // regardless. Aliases the wanted lockfile *does* track are
        // skipped — those belong to the lockfile passes (and their
        // dedupe decisions). See [`crate::link_manifest_link_deps`].
        // These are importer symlinks like any other, so
        // `virtualStoreOnly` skips them too.
        if !config.virtual_store_only {
            crate::link_manifest_link_deps::<Reporter>(
                &workspace_root,
                &materialized_project_manifests,
                fresh_lockfile.as_ref().or(lockfile).and_then(|lockfile| {
                    (!lockfile.importers.is_empty()).then_some(&lockfile.importers)
                }),
                // Honor a `modulesDir` override the same way the
                // lockfile-driven symlink pass does.
                config
                    .modules_dir
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("node_modules")),
                &crate::shim_extra_node_paths(config, node_linker),
            )
            .map_err(InstallError::LinkManifestLinkDeps)?;
        }

        // `Stage::ImportingDone` is emitted inside the install paths
        // (`InstallFrozenLockfile` between symlink and build, and
        // `InstallWithFreshLockfile` after the writer task) so that any
        // subsequent `pnpm:lifecycle` events render after the import
        // progress display has closed.

        // Remove surplus virtual-store directories the wanted lockfile
        // no longer references, throttled by `modulesCacheMaxAge`.
        // The wanted lockfile is `fresh_lockfile` on the resolve path and
        // `lockfile` on the frozen path; its `snapshots:` keys name the
        // virtual-store subdirectories that must survive.
        // A genuine read/parse failure (not `NotFound`) is treated as
        // "no prior manifest" — the safe direction (prune + fresh
        // `prunedAt`) — but logged rather than silently swallowed.
        let prior_modules = modules_manifest;
        let now = SystemTime::now();
        let effective_virtual_store_dir = config.effective_virtual_store_dir();
        // Decide "this is the global store" from the resolved paths, not
        // the `enableGlobalVirtualStore` flag alone: the global store is
        // shared across projects, so a config that points `virtualStoreDir`
        // at it must not be pruned even when the flag is off.
        let is_global_virtual_store = crate::prune_virtual_store::same_dir(
            effective_virtual_store_dir,
            &config.global_virtual_store_dir,
        );
        // `did_prune` tracks whether the sweep actually ran (enumerated the
        // store), not just whether the throttle allowed it. It stays false
        // when there is no wanted lockfile to derive the needed set from
        // (e.g. `config.lockfile == false` leaves both `fresh_lockfile` and
        // a loaded `lockfile` absent), when the target is refused as unsafe,
        // or when enumeration failed. `prunedAt` must not advance on a run
        // where nothing was swept, or the next real sweep is throttled off
        // for `modulesCacheMaxAge`.
        let did_prune = if crate::prune_virtual_store::should_prune_virtual_store(
            is_global_virtual_store,
            prior_modules.as_ref().map(|modules| modules.pruned_at.as_str()),
            config.modules_cache_max_age,
            now,
        ) {
            match materialized_current_lockfile.as_ref() {
                // Sweep the canonicalized prune target returned by the
                // containment check, never the raw configured path: deleting
                // from the validated path closes the time-of-check/time-of-use
                // gap a symlink swap would otherwise open.
                Some(wanted) => {
                    if let Some(prune_dir) = crate::prune_virtual_store::prune_target_within_modules(
                        effective_virtual_store_dir,
                        &config.modules_dir,
                    ) {
                        crate::prune_virtual_store::prune_virtual_store(
                            &prune_dir,
                            wanted.snapshots.iter().flat_map(|snapshots| snapshots.keys()),
                            &install_skipped,
                            config.virtual_store_dir_max_length as usize,
                        )
                        .is_some()
                    } else {
                        // A wanted lockfile exists but the store path is unsafe
                        // (escapes node_modules); refuse the destructive sweep.
                        tracing::warn!(
                            virtual_store_dir = %effective_virtual_store_dir.display(),
                            modules_dir = %config.modules_dir.display(),
                            "skipping virtual-store prune: the virtual store is not inside node_modules",
                        );
                        false
                    }
                }
                None => false,
            }
        } else {
            false
        };

        // Stamp `prunedAt` only when the sweep ran (or there was no prior
        // `.modules.yaml`); otherwise preserve the recorded timestamp so
        // the throttle keeps counting from the last real prune.
        let pruned_at = match (&prior_modules, did_prune) {
            (Some(prior), false) => prior.pruned_at.clone(),
            _ => httpdate::fmt_http_date(now),
        };

        // Write `node_modules/.modules.yaml`. Fires after
        // `importing_done` and before the closing `pnpm:summary` emit.
        // The manifest records the resolved directory layout, hoist
        // patterns, included dependency groups, store dir, and registries
        // so a later install (or another tool) can detect a layout change
        // and prune accordingly.
        // The projects whose own install scripts `--ignore-scripts`
        // skipped are owed a build just like the dependencies the build
        // phase deferred, and are recorded by importer id.
        let deferred_projects = config.ignore_scripts.then(|| {
            materialized_project_manifests
                .iter()
                .filter(|(project_dir, manifest)| {
                    project_requires_lifecycle_scripts(project_dir, manifest)
                })
                .map(|(project_dir, _)| {
                    pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir)
                })
                .collect::<Vec<_>>()
        });
        let previous_pending_builds =
            prior_modules.map_or(&[][..], |modules| modules.pending_builds.as_slice());
        // The build phase settles a dependency only when it actually
        // rebuilt it, so a `pnpm rebuild --pending` that the policy still
        // blocks (`allowBuilds: None`/`false`) leaves the debt in place.
        // Reuse the same policy `BuildModules` ran under; on a rebuild a
        // selected, approved dependency always runs (force-rebuild
        // bypasses the side-effects cache gate), so policy approval is a
        // faithful stand-in for "was rebuilt".
        let rebuild_build_policy =
            rebuild.as_ref().and_then(|_| crate::AllowBuildPolicy::from_config(config).ok());
        let pending_builds = merge_pending_builds(
            previous_pending_builds,
            deferred_projects.into_iter().flatten().chain(deferred_builds),
            materialized_current_lockfile.as_ref(),
            rebuild.as_ref(),
            rebuild_build_policy.as_ref(),
        );

        let mut next_modules = build_modules_manifest(
            config,
            node_linker,
            included,
            hoisted_dependencies,
            hoisted_locations,
            injected_deps,
            &install_skipped,
            &ignored_builds,
            pending_builds,
            pruned_at,
        );
        if filtered_install
            && !matches!(node_linker, NodeLinker::Hoisted)
            && !is_inconsistent
            && let (Some(previous), Some(current), Some(selected)) = (
                previous_modules_metadata.as_ref(),
                materialized_current_lockfile.as_ref(),
                selected_current_lockfile.as_ref(),
            )
        {
            merge_filtered_modules_metadata(&mut next_modules, previous, current, selected);
        }
        write_modules_manifest::<Host>(&config.modules_dir, next_modules)
            .map_err(InstallError::WriteModules)?;

        // Write `<virtual_store_dir>/lock.yaml`. Captures what was
        // actually materialized so the next install can diff each
        // snapshot against it and skip the unchanged
        // slots. Persist *after* `write_modules_manifest` succeeds so
        // a manifest failure can't leave a fresh current-lockfile
        // pointing at incomplete install state — the next frozen
        // reinstall would otherwise diff against a graph that never
        // finished committing (review on <https://github.com/pnpm/pacquet/pull/442>).
        //
        // A filtered isolated/PnP install merges its newly materialized
        // closure into compatible prior current state, while a hoisted
        // install records the full shared graph it materialized. This
        // keeps the file aligned with physical state without discarding
        // unselected slots that remain on disk.
        if let Some(lockfile) = materialized_current_lockfile.as_ref() {
            // Filter the wanted lockfile down to the snapshots that
            // were actually materialized: dep maps the user excluded
            // (`--no-optional`, `--no-dev`) plus snapshots the
            // install-time skip set dropped (installability, fetch
            // failure, `--no-optional`-only entries). The next install
            // diffs against this filtered shape so dropped snapshots
            // aren't mistaken for already-done work.
            lockfile
                .save_current_to_virtual_store_dir(&config.virtual_store_dir)
                .map_err(InstallError::SaveCurrentLockfile)?;
        }

        // Regenerate `pnpm-lock.yaml` from the synthesized snapshot when
        // the wanted lockfile was reconstructed from
        // `<virtual_store_dir>/lock.yaml`. The no-op short-circuit above
        // handles the common case; this branch covers the rare path where
        // `.modules.yaml` was wiped or inconsistent and the frozen install
        // had to relink.
        if lockfile_synthesized_from_current
            && config.lockfile
            && let Some(synthesized) = synthesized_lockfile.as_ref()
        {
            synthesized
                .save_to_path(&workspace_root.join(Lockfile::FILE_NAME))
                .map_err(InstallError::SaveWantedLockfile)?;
        }

        // Run each workspace project's own lifecycle scripts now that
        // the dependency graph is materialized, bins are linked, and
        // `.modules.yaml` / the current lockfile are written. The
        // `pnpm:lifecycle` events these scripts produce render before
        // the closing `pnpm:summary` below.
        //
        // Skipped for partial installs (`pacquet add`): pnpm filters
        // to `mutation === 'install'` so a named install does not fire
        // the project's own scripts (see [`Install::is_full_install`]).
        //
        // Also skipped under `--ignore-scripts`: pnpm suppresses the
        // project's own lifecycle scripts alongside dependency build
        // scripts when `ignoreScripts` is set.
        //
        // And under `virtualStoreOnly`, which stops before any linking
        // the project's scripts would expect to find in place.
        //
        // A `pnpm rebuild --pending` is the exception to the
        // full-install gate: it is not a full install, but the projects
        // it names are exactly the ones whose scripts an earlier
        // `--ignore-scripts` install deferred, and running them is what
        // lets the install drop those entries from `pendingBuilds` (see
        // [`merge_pending_builds`]).
        let projects_to_run: Vec<(std::path::PathBuf, &PackageManifest)> = if config.ignore_scripts
            || config.virtual_store_only
        {
            Vec::new()
        } else if is_full_install {
            materialized_project_manifests.clone()
        } else if let Some(rebuild) = rebuild.as_ref() {
            materialized_project_manifests
                .iter()
                .filter(|(project_dir, _)| {
                    let importer_id =
                        pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir);
                    rebuild.pending_projects.contains(&importer_id)
                })
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        if !projects_to_run.is_empty() {
            let project_groups = order_project_lifecycle_groups(
                &projects_to_run,
                selection.as_ref().map(|selection| selection.ordered_groups),
                &workspace_root,
                materialized_current_lockfile.as_ref(),
            )?;
            if !project_groups.is_empty() {
                run_projects_lifecycle_scripts::<Reporter>(
                    &project_groups,
                    config,
                    node_linker,
                    &workspace_root,
                )?;
            }
            if let Some(rebuild) = rebuild.as_ref() {
                drain_settled_projects::<Host>(&config.modules_dir, &rebuild.pending_projects)?;
            }
        }

        // Write `node_modules/.pnpm-workspace-state-v1.json`.
        // pnpm's `verifyDepsBeforeRun` gate bails to "outdated" the
        // moment this file is missing, forcing `pnpm install` to rerun.
        // Writing it after both the `.modules.yaml` and the current
        // lockfile succeed keeps the file pointing at a fully committed
        // install.
        update_workspace_state(
            &workspace_root,
            &build_workspace_state(
                &workspace_root,
                config,
                node_linker,
                included,
                supported_architectures.as_ref(),
                &catalogs,
                &project_manifests,
                filtered_install,
            ),
        )
        .map_err(InstallError::WriteWorkspaceState)?;

        // `pnpm:summary` closes the install and lets the reporter render
        // the accumulated `pnpm:root` events as a "+N -M" block. Must
        // come after `importing_done`.
        Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));

        // When `strictDepBuilds` is on (the default), an install that
        // blocked any dependency build script fails with
        // `ERR_PNPM_IGNORED_BUILDS` *after* the artifacts are written, so
        // the package is still added/installed and the user approves the
        // builds and reinstalls.
        if config.strict_dep_builds && !ignored_builds.is_empty() {
            return Err(InstallError::IgnoredBuilds { package_names: ignored_builds });
        }

        Ok(())
    }
}

/// Run every gate the frozen-lockfile dispatch consults before
/// committing to materializing `node_modules` from `lockfile`:
/// `pnpm.overrides` parsing, the settings-drift check
/// ([`pacquet_lockfile::check_lockfile_settings`]), and the
/// per-importer manifest specifier check
/// ([`pacquet_lockfile::satisfies_package_manifest`]).
///
/// Shared between dispatch states 1 and 2 so the explicit
/// `--frozen-lockfile` flag and the implicit `preferFrozenLockfile:
/// true` fast path agree on what "lockfile is up to date" means.
/// Callers in state 1 surface any `Err` as [`InstallError`]; callers
/// in state 2 treat a stale-lockfile `Err` as fall-through to the
/// fresh-resolve path (and surface the rest as fatal — see the
/// `From<FreshnessCheckError> for InstallError` impl below).
///
/// `ignore_manifest_check` skips the per-importer specifier gate.
/// The pnpm CLI passes it when delegating materialization through
/// `configDependencies`: pnpm has just resolved the tree and written
/// the lockfile, but hasn't yet written the post-mutation
/// `package.json` to disk, so the freshness check would always fire
/// on `pnpm up` / `add` / `remove`. Settings drift (`overrides`,
/// `ignoredOptionalDependencies`) still runs.
fn check_lockfile_freshness(
    lockfile: &Lockfile,
    manifest_freshness_inputs: &[(String, &PackageManifest)],
    config: &Config,
    catalogs: &Catalogs,
    ignore_manifest_check: bool,
    allow_missing_dependency_free_importers: bool,
) -> Result<(), FreshnessCheckError> {
    let parsed_overrides_opt = parse_config_overrides(config, catalogs)?;
    check_lockfile_settings_drift(lockfile, config, catalogs, parsed_overrides_opt.as_deref())?;

    if ignore_manifest_check {
        return Ok(());
    }

    let ignored_optional_matcher = pacquet_config::matcher::create_matcher(
        config.ignored_optional_dependencies.as_deref().unwrap_or_default(),
    );
    for (importer_id, manifest) in manifest_freshness_inputs {
        if allow_missing_dependency_free_importers
            && !lockfile.importers.contains_key(importer_id)
            && !manifest_has_effective_dependencies(manifest, &ignored_optional_matcher)
        {
            continue;
        }
        check_importer_satisfies(
            lockfile,
            manifest,
            importer_id,
            config,
            &ignored_optional_matcher,
            parsed_overrides_opt.as_deref(),
        )?;
    }
    Ok(())
}

/// Parse `pnpm.overrides` from the config. Values can use the
/// `catalog:` protocol, which pnpm resolves against the workspace's
/// catalogs *before* writing them to `pnpm-lock.yaml#overrides` —
/// resolving here keeps an override declared as `"foo": "catalog:"`
/// comparable to the lockfile's already-resolved `"foo": "<concrete>"`.
pub(crate) fn parse_config_overrides(
    config: &Config,
    catalogs: &Catalogs,
) -> Result<Option<Vec<pacquet_config_parse_overrides::VersionOverride>>, FreshnessCheckError> {
    match config.overrides.as_ref() {
        Some(map) if !map.is_empty() => Ok(Some(
            pacquet_config_parse_overrides::parse_overrides_iter(map.iter(), catalogs)
                .map_err(FreshnessCheckError::InvalidOverrides)?,
        )),
        _ => Ok(None),
    }
}

/// Outdated-settings gate (umbrella <https://github.com/pnpm/pacquet/issues/434> slice 7): check
/// `ignoredOptionalDependencies` + `overrides` +
/// `packageExtensionsChecksum` drift between the lockfile-recorded
/// values and the current config before the per-importer specifier
/// check.
pub(crate) fn check_lockfile_settings_drift(
    lockfile: &Lockfile,
    config: &Config,
    catalogs: &Catalogs,
    parsed_overrides: Option<&[pacquet_config_parse_overrides::VersionOverride]>,
) -> Result<(), FreshnessCheckError> {
    let overrides_map: Option<std::collections::HashMap<String, String>> =
        parsed_overrides.map(pacquet_config_parse_overrides::create_overrides_map_from_parsed);
    let package_extensions_checksum =
        crate::install_with_fresh_lockfile::compute_package_extensions_checksum(config);
    // `calcPatchHashes(opts.patchedDependencies)` — reading the patch
    // files here lets `check_lockfile_settings` catch an edited patch
    // whose hash (and thus its `(patch_hash=...)` depPath suffix) drifted
    // from what the lockfile recorded.
    let patched_dependency_hashes =
        config.patched_dependency_hashes().map_err(FreshnessCheckError::CalcPatchHashes)?;
    pacquet_lockfile::check_lockfile_settings_with_catalogs(
        lockfile,
        pacquet_lockfile::LockfileSettingsCheck {
            catalogs,
            overrides: overrides_map.as_ref(),
            package_extensions_checksum: package_extensions_checksum.as_deref(),
            ignored_optional_dependencies: config.ignored_optional_dependencies.as_deref(),
            patched_dependencies: patched_dependency_hashes.as_ref(),
            inject_workspace_packages: config.inject_workspace_packages,
            peers_suffix_max_length: config.peers_suffix_max_length,
        },
    )
    .map_err(FreshnessCheckError::Stale)
}

/// Per-importer slice of the freshness gate: the manifest of the
/// project at `importer_id` must still be satisfied by the lockfile's
/// importer snapshot.
pub(crate) fn check_importer_satisfies(
    lockfile: &Lockfile,
    manifest: &PackageManifest,
    importer_id: &str,
    config: &Config,
    ignored_optional_matcher: &pacquet_config::matcher::Matcher,
    parsed_overrides: Option<&[pacquet_config_parse_overrides::VersionOverride]>,
) -> Result<(), FreshnessCheckError> {
    let importer = lockfile
        .importers
        .get(importer_id)
        .ok_or_else(|| FreshnessCheckError::NoImporter { importer_id: importer_id.to_string() })?;

    // Apply `pnpm.overrides` to a *cloned* manifest before the
    // per-importer specifier check so the lockfile's specifiers —
    // written with overrides already applied — match the on-disk
    // manifest's deps. The caller's manifest stays pristine since the
    // override pass conceptually returns a new manifest
    // from the perspective of every consumer downstream of the
    // resolver.
    // `auto_install_peers` is folded into `satisfies_package_manifest`
    // itself, so the manifest is cloned here only for the two mutations the
    // comparison needs done up front: applying `pnpm.overrides` and dropping
    // `link:` deps under `exclude_links_from_lockfile`.
    let normalized_manifest_holder;
    let manifest_for_freshness: &PackageManifest = if parsed_overrides.is_some()
        || config.exclude_links_from_lockfile
    {
        let root_dir = manifest.path().parent().unwrap_or_else(|| Path::new("."));
        normalized_manifest_holder = {
            let mut cloned: PackageManifest = manifest.clone();
            if let Some(parsed) = parsed_overrides {
                crate::VersionsOverrider::new(parsed, root_dir).apply(&mut cloned, Some(root_dir));
            }
            if config.exclude_links_from_lockfile {
                exclude_linked_dependencies(&mut cloned);
            }
            cloned
        };
        &normalized_manifest_holder
    } else {
        manifest
    };

    // Build the `ignoredOptionalDependencies` filter set: iterate
    // `manifest.optionalDependencies` and delete matches from BOTH the
    // `optional` and `dependencies` maps. A name only present in
    // `dependencies` that happens to match the
    // pattern is NOT removed — set-based ("name was in
    // optionalDependencies AND matched") rather than pure pattern
    // matching. `devDependencies` is untouched on purpose; the group
    // gate inside `satisfies_package_manifest` enforces that.
    let ignored_set =
        ignored_optional_dependency_names(manifest_for_freshness, ignored_optional_matcher);
    let is_ignored_optional: &dyn Fn(&str) -> bool = &|name: &str| ignored_set.contains(name);

    satisfies_package_manifest(
        importer,
        manifest_for_freshness,
        config.auto_install_peers,
        is_ignored_optional,
    )
    .map_err(FreshnessCheckError::Stale)
}

fn ignored_optional_dependency_names(
    manifest: &PackageManifest,
    matcher: &pacquet_config::matcher::Matcher,
) -> std::collections::HashSet<String> {
    manifest
        .dependencies([pacquet_package_manifest::DependencyGroup::Optional])
        .filter(|(name, _)| matcher.matches(name))
        .map(|(name, _)| name.to_string())
        .collect()
}

fn manifest_has_effective_dependencies(
    manifest: &PackageManifest,
    ignored_optional_matcher: &pacquet_config::matcher::Matcher,
) -> bool {
    if manifest.dependencies([pacquet_package_manifest::DependencyGroup::Dev]).next().is_some() {
        return true;
    }
    let ignored = ignored_optional_dependency_names(manifest, ignored_optional_matcher);
    manifest
        .dependencies([
            pacquet_package_manifest::DependencyGroup::Prod,
            pacquet_package_manifest::DependencyGroup::Optional,
        ])
        .any(|(name, _)| !ignored.contains(name))
}

fn exclude_linked_dependencies(manifest: &mut PackageManifest) {
    let Some(manifest) = manifest.value_mut().as_object_mut() else {
        return;
    };
    for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional] {
        let group: &str = group.into();
        if let Some(dependencies) =
            manifest.get_mut(group).and_then(serde_json::Value::as_object_mut)
        {
            dependencies.retain(|_, specifier| {
                let Some(specifier) = specifier.as_str() else {
                    return true;
                };
                !specifier.starts_with("link:")
            });
        }
    }
}

/// Outcome of [`check_lockfile_freshness`]. Splits "user
/// configuration is malformed" (always fatal) from "lockfile is stale"
/// (fatal for `--frozen-lockfile`, fall-through to the fresh-resolve
/// path under `preferFrozenLockfile: true`).
#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum FreshnessCheckError {
    /// The lockfile has no entry for the root importer.
    #[display(
        r#"Cannot install with "frozen-lockfile" because pnpm-lock.yaml has no `importers["{importer_id}"]` entry. Regenerate the lockfile with `pnpm install --lockfile-only`."#
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_NO_IMPORTER))]
    NoImporter { importer_id: String },

    /// A value in `pnpm.overrides` couldn't be parsed.
    #[diagnostic(transparent)]
    InvalidOverrides(#[error(source)] pacquet_config_parse_overrides::ParseOverridesError),

    /// A configured `patchedDependencies` patch file couldn't be read
    /// or hashed while computing the map to compare against the
    /// lockfile.
    #[diagnostic(transparent)]
    CalcPatchHashes(#[error(source)] pacquet_patching::CalcPatchHashError),

    /// `pnpm-lock.yaml` doesn't match the on-disk `package.json` /
    /// current settings.
    #[display("{_0}")]
    Stale(#[error(not(source))] StalenessReason),
}

impl From<FreshnessCheckError> for InstallError {
    fn from(error: FreshnessCheckError) -> InstallError {
        match error {
            FreshnessCheckError::NoImporter { importer_id } => {
                InstallError::NoImporter { importer_id }
            }
            FreshnessCheckError::InvalidOverrides(inner) => InstallError::InvalidOverrides(inner),
            FreshnessCheckError::CalcPatchHashes(inner) => InstallError::WithFreshLockfile(
                InstallWithFreshLockfileError::CalcPatchHashes(inner),
            ),
            FreshnessCheckError::Stale(reason) => InstallError::OutdatedLockfile { reason },
        }
    }
}

/// Translate pacquet's [`Config::node_linker`] into the
/// [`pacquet_modules_yaml::NodeLinker`] enum used on disk. The two
/// enums share the same variant set (`isolated`, `hoisted`, `pnp`),
/// the values of the `nodeLinker` string.
fn map_node_linker(linker: NodeLinker) -> ModulesNodeLinker {
    match linker {
        NodeLinker::Isolated => ModulesNodeLinker::Isolated,
        NodeLinker::Hoisted => ModulesNodeLinker::Hoisted,
        NodeLinker::Pnp => ModulesNodeLinker::Pnp,
    }
}

/// Whether a parsed `.modules.yaml` records the same layout settings
/// (`nodeLinker`, hoist patterns, store / virtual-store paths,
/// `virtualStoreDirMaxLength`, included dep groups, layout version) the
/// current install would produce. A mismatch disqualifies the no-op
/// short-circuit.
///
/// Takes the already-parsed [`Modules`] so the up-to-date fast path can
/// share one parse across the consistency, newly-allowed, and
/// unapproved-ignored checks.
fn modules_consistent_with(
    modules: &pacquet_modules_yaml::ModulesLayout,
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
) -> bool {
    // A `virtualStoreOnly` install populates the virtual store and stops,
    // so the modules directory it leaves behind has no importer symlinks,
    // bins, or hoisted packages. It can never satisfy an ordinary
    // install, however well its recorded settings line up — the no-op
    // short-circuit would leave the linking permanently undone.
    if modules.virtual_store_only == Some(true) && !config.virtual_store_only {
        return false;
    }
    modules.included == included && modules_layout_consistent_with(modules, config, node_linker)
}

/// The subset of [`modules_consistent_with`] that, when it drifts, requires
/// **wiping and recreating** `node_modules`. It deliberately excludes
/// `included`: a `--prod`<->full switch is satisfied by relinking the
/// newly-selected groups plus the targeted removal of the now-excluded
/// ones ([`crate::prune_direct_deps_excluded_by_groups`]), not by
/// deleting the directory. pnpm never purges the root project's
/// `node_modules` for an included mismatch — its `validateModules` only
/// does so for non-root importers (the `lockfileDir !== rootDir` check
/// in `pnpm11/installing/deps-installer/src/install/validateModules.ts`)
/// — so purging here would destroy the user's own non-pnpm entries (a
/// vendored directory, stray files) on a routine flag change. The
/// up-to-date fast path still compares `included` via
/// [`modules_consistent_with`], so the relink it triggers stays correct.
/// On-disk probe backing the frozen no-op short-circuit: the
/// short-circuit skips the materialization walk entirely, so it must
/// first prove the tree it would skip is still whole — pnpm's headless
/// path stats every package dir on every run, which is what repairs a
/// hand-deleted package. One metadata call per snapshot slot plus one
/// per direct-dep link; any missing entry falls through to the full
/// frozen path, which re-materializes it (emitting
/// `pnpm:_broken_node_modules`).
///
/// Under a global virtual store the slot paths depend on graph hashes
/// the short-circuit doesn't compute, and the hoisted linker has no
/// virtual-store slots; both probe only the importer links.
fn frozen_tree_intact(
    wanted: &Lockfile,
    modules: &pacquet_modules_yaml::ModulesLayout,
    config: &Config,
    workspace_root: &Path,
    node_linker: NodeLinker,
) -> bool {
    if matches!(node_linker, NodeLinker::Pnp) && !workspace_root.join(crate::PNP_FILENAME).is_file()
    {
        return false;
    }
    let skipped = crate::SkippedSnapshots::from_strings(&modules.skipped);
    let probe_slots =
        !matches!(node_linker, NodeLinker::Hoisted) && !config.enable_global_virtual_store;
    if probe_slots && let Some(snapshots) = wanted.snapshots.as_ref() {
        let layout = crate::VirtualStoreLayout::legacy(
            config.virtual_store_dir.clone(),
            config.virtual_store_dir_max_length as usize,
        );
        let all_slots_present = snapshots.keys().all(|key| {
            if skipped.contains(key) {
                return true;
            }
            // The name is lockfile-controlled: join it with the same
            // traversal-rejecting helper the linkers use, and treat a
            // malformed name as not-intact so the full path's
            // structural lockfile gate rejects it.
            let slot_node_modules = layout.slot_dir(key).join("node_modules");
            match crate::safe_join_modules_dir::safe_join_modules_dir(
                &slot_node_modules,
                &key.name.to_string(),
            ) {
                Ok(dir) => dir.is_dir(),
                Err(_) => false,
            }
        });
        if !all_slots_present {
            return false;
        }
    }
    if !config.symlink {
        return probe_slots;
    }
    let groups = crate::prune_direct_deps::selected_groups(modules.included);
    let modules_dir_name: &std::ffi::OsStr =
        config.modules_dir.file_name().unwrap_or_else(|| std::ffi::OsStr::new("node_modules"));
    wanted.importers.iter().all(|(importer_id, snapshot)| {
        if crate::symlink_direct_dependencies::validate_importer_id(importer_id).is_err() {
            return true;
        }
        let modules_dir =
            crate::symlink_direct_dependencies::importer_root_dir(workspace_root, importer_id)
                .join(modules_dir_name);
        crate::symlink_direct_dependencies::direct_dep_names_for_importer(
            snapshot,
            groups.iter().copied(),
            &skipped,
            false,
        )
        .iter()
        .all(|name| {
            match crate::safe_join_modules_dir::safe_join_modules_dir(&modules_dir, name) {
                // `metadata` follows the link, so a dangling direct-dep
                // symlink (a wiped GVS store, a hand-deleted target)
                // reads as broken and falls through to the repairing
                // full path.
                Ok(link) => std::fs::metadata(link).is_ok(),
                // A malformed alias never probes the disk; the full
                // path rejects it with its own typed error.
                Err(_) => true,
            }
        })
    })
}

/// Whether a GVS install can own slots whose interrupted build or patch
/// application must be recovered from `.pnpm-needs-build`.
///
/// The marker is shared store state, so neither optimistic workspace state nor
/// the frozen importer's symlinks can prove it absent. Only configurations
/// capable of acting on one need to leave those no-op paths.
fn gvs_build_markers_may_require_recovery(config: &Config) -> bool {
    config.enable_global_virtual_store
        && (config.dangerously_allow_all_builds
            || config.allow_builds.values().any(|allowed| *allowed)
            || config.patched_dependencies.as_ref().is_some_and(|patches| !patches.is_empty()))
}

/// Probe the GVS name/version directories that can contain an actionable
/// marker for this lockfile. Hash directories are enumerated rather than
/// recomputed because buildable slots include the runtime engine in their
/// graph hash, which the pre-runtime fast path deliberately has not resolved.
fn gvs_build_marker_present(wanted: &Lockfile, config: &Config) -> bool {
    if !gvs_build_markers_may_require_recovery(config) {
        return false;
    }
    let Ok(policy) = crate::AllowBuildPolicy::from_config(config) else {
        return true;
    };
    let layout = crate::VirtualStoreLayout::new(
        config,
        None,
        wanted.snapshots.as_ref(),
        wanted.packages.as_ref(),
        Some(&policy),
    );
    if crate::validate_virtual_store_slot_containment(wanted.snapshots.as_ref(), &layout).is_err() {
        return true;
    }
    let Some(snapshots) = wanted.snapshots.as_ref() else {
        return false;
    };

    let mut visited_version_dirs = HashSet::new();
    for snapshot_key in snapshots.keys() {
        let can_recover = crate::snapshot_has_patch(snapshot_key)
            || policy.check(&snapshot_key.without_peer().to_string()) == Some(true);
        if !can_recover {
            continue;
        }
        let Some(version_dir) = layout.slot_dir(snapshot_key).parent().map(Path::to_path_buf)
        else {
            return true;
        };
        if !visited_version_dirs.insert(version_dir.clone()) {
            continue;
        }
        let hash_dirs = match std::fs::read_dir(&version_dir) {
            Ok(hash_dirs) => hash_dirs,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return true,
        };
        for hash_dir in hash_dirs {
            let Ok(hash_dir) = hash_dir else {
                return true;
            };
            let Ok(file_type) = hash_dir.file_type() else {
                return true;
            };
            if !file_type.is_dir() {
                continue;
            }
            let Ok(pkg_dir) = crate::safe_join_modules_dir::safe_join_modules_dir(
                &hash_dir.path().join("node_modules"),
                &snapshot_key.name.to_string(),
            ) else {
                return true;
            };
            if pkg_dir.join(crate::NEEDS_BUILD_MARKER).is_file() {
                return true;
            }
        }
    }
    false
}

/// The `validateModules` half pacquet enforces: when the mutation is
/// not a plain install (upstream `installsOnly === false`), a drift in
/// the persisted layout settings fails with the upstream `*_DIFF`
/// error instead of silently recreating the modules directory. Check
/// order matches upstream `validateModules`. Drift in the fields this
/// does not cover (store dir, node linker, layout version) still takes
/// the recreate path.
fn check_modules_settings_diff(
    modules: &pacquet_modules_yaml::ModulesLayout,
    config: &Config,
) -> Result<(), InstallError> {
    if modules.virtual_store_dir_max_length != config.virtual_store_dir_max_length {
        return Err(InstallError::VirtualStoreDirMaxLengthDiff);
    }
    if normalized_pattern(modules.public_hoist_pattern.as_deref())
        != normalized_pattern(config.public_hoist_pattern.as_deref())
    {
        return Err(InstallError::PublicHoistPatternDiff);
    }
    if normalized_pattern(modules.hoist_pattern.as_deref())
        != normalized_pattern(config.hoist_pattern.as_deref())
    {
        return Err(InstallError::HoistPatternDiff);
    }
    Ok(())
}

/// Upstream compares patterns with `?? []`: `None` and an empty list
/// are the same disabled state.
fn normalized_pattern(pattern: Option<&[String]>) -> &[String] {
    pattern.unwrap_or(&[])
}

fn modules_layout_consistent_with(
    modules: &pacquet_modules_yaml::ModulesLayout,
    config: &Config,
    node_linker: NodeLinker,
) -> bool {
    // A `virtualStoreOnly` install (`pnpm fetch`) records empty hoist
    // patterns because it deliberately did no hoisting. Diffing those
    // against the follow-up install's real patterns would read as drift
    // and purge the directory the fetch just populated, so the
    // comparison is skipped and the follow-up completes the linking
    // instead.
    // Patterns compare normalized (upstream's `?? []`): `None` and an
    // empty list are the same disabled state, so the pair must not read
    // as layout drift — a purge every install for `hoistPattern: []`
    // projects, and a spurious `*_DIFF` error for `add` / `remove`. A
    // `virtualStoreOnly` install records empty patterns deliberately, so
    // it skips the comparison entirely and lets the follow-up install
    // complete the linking instead of purging.
    let hoist_patterns_match = modules.virtual_store_only == Some(true)
        || (normalized_pattern(modules.hoist_pattern.as_deref())
            == normalized_pattern(config.hoist_pattern.as_deref())
            && normalized_pattern(modules.public_hoist_pattern.as_deref())
                == normalized_pattern(config.public_hoist_pattern.as_deref()));
    modules.layout_version == Some(LayoutVersion)
        && modules.node_linker == Some(map_node_linker(node_linker))
        && hoist_patterns_match
        && modules.virtual_store_dir_max_length == config.virtual_store_dir_max_length
        && modules.store_dir == config.store_dir.display().to_string()
        && modules.virtual_store_dir
            == config.effective_virtual_store_dir().to_string_lossy().as_ref()
}

/// Whether `.modules.yaml` records any ignored build that the current
/// `allowBuilds` policy now allows.
///
/// When `true`, the frozen no-op fast path must not short-circuit: the
/// install has to rebuild the newly-allowed package, re-running the
/// builds an `allowBuilds` change un-ignored even on an otherwise
/// up-to-date install. pacquet achieves this by letting the full frozen
/// install run, whose `BuildModules` re-evaluates the policy and
/// rebuilds the now-allowed package (already built deps are skipped by
/// the side-effects-cache `is_built` gate).
fn has_newly_allowed_ignored_builds(
    modules: &pacquet_modules_yaml::ModulesLayout,
    config: &Config,
) -> bool {
    let Some(ignored) = modules.ignored_builds.as_ref().filter(|set| !set.is_empty()) else {
        return false;
    };
    // A malformed `allowBuilds` can't be evaluated here; let the full
    // install run so it surfaces the real error instead of silently
    // staying on the fast path.
    let Ok(policy) = crate::AllowBuildPolicy::from_config(config) else {
        return true;
    };
    ignored.iter().any(|dep_path| policy.check(dep_path.as_str()) == Some(true))
}

/// Whether the current `allowBuilds` policy withdraws an approval that
/// `.modules.yaml` recorded, leaving the package undecided again.
///
/// The counterpart to [`has_newly_allowed_ignored_builds`]: a build the
/// previous install ran is absent from `ignoredBuilds`, so nothing else
/// on the frozen no-op fast path notices it is no longer approved
/// (<https://github.com/pnpm/pnpm/issues/11035>).
///
/// Only a withdrawal to *undecided* counts. An entry the user flipped to
/// an explicit `false` is silently skipped rather than reported, so it
/// leaves the fast path intact — matching `BuildModules`.
fn has_revoked_allowed_builds(
    modules: &pacquet_modules_yaml::ModulesLayout,
    config: &Config,
) -> bool {
    let Some(recorded) = modules.allow_builds.as_ref() else { return false };
    recorded
        .iter()
        .filter(|(_, value)| matches!(value, pacquet_modules_yaml::AllowBuildValue::Bool(true)))
        .any(|(spec, _)| !config.allow_builds.contains_key(spec))
}

/// The sorted `name@version` keys `.modules.yaml` recorded as ignored
/// builds that the current `allowBuilds` policy still leaves unapproved
/// (`None`), or `None` when there are none.
///
/// The up-to-date fast paths use this to keep `strictDepBuilds`
/// enforced across reruns: `ignoredBuilds` is seeded from `.modules.yaml`
/// on the up-to-date path and the ignored-builds check still throws, so a
/// rerun after an `ERR_PNPM_IGNORED_BUILDS` failure must not exit 0.
/// Packages a later policy explicitly denies (`Some(false)`) are excluded
/// — those are silently skipped, never reported — matching a full
/// install's `BuildModules`. Newly-allowed packages are handled
/// by [`has_newly_allowed_ignored_builds`], which skips the fast path.
///
/// A malformed `allowBuilds` spec surfaces as `Err` (e.g.
/// `ERR_PNPM_INVALID_VERSION_UNION`) rather than being swallowed: the
/// fast-path callers fall through to the full install on `Err`, which
/// re-evaluates the policy and reports the real error.
fn unapproved_recorded_ignored_builds(
    modules: &pacquet_modules_yaml::ModulesLayout,
    config: &Config,
) -> Result<Option<Vec<String>>, pacquet_config::version_policy::VersionPolicyError> {
    let Some(ignored) = modules.ignored_builds.as_ref().filter(|set| !set.is_empty()) else {
        return Ok(None);
    };
    let policy = crate::AllowBuildPolicy::from_config(config)?;
    let mut names: Vec<String> = ignored
        .iter()
        .filter(|dep_path| policy.check(dep_path.as_str()).is_none())
        .map(|dep_path| dep_path.as_str().to_string())
        .collect();
    names.sort();
    Ok((!names.is_empty()).then_some(names))
}

/// Assemble the [`Modules`] payload for [`write_modules_manifest`].
///
/// `hoistedDependencies` is produced by the isolated-linker hoist
/// pass in [`crate::InstallFrozenLockfile::run`] and threaded in
/// here — empty for the no-lockfile path, for installs where both
/// hoist patterns are `None`, and under `nodeLinker: hoisted` (the
/// hoisted linker uses `hoisted_locations` instead). Persisting it
/// lets a subsequent install detect a hoist pattern change and
/// re-hoist appropriately (the partial-install path tracked at
/// pnpm/pacquet#433 will consume it; today every install does the
/// full hoist anyway).
///
/// `hoisted_locations` is the per-depPath list of lockfile-relative
/// directory paths the hoisted linker placed each package at. Empty
/// for the isolated linker (the field is hoisted-only on disk and
/// only meaningful when `nodeLinker: hoisted`). Persisted into
/// [`Modules::hoisted_locations`] when non-empty so the next
/// install's walker can short-circuit re-fetching packages already
/// present on disk and the rebuild path can locate every hoisted
/// directory; absent persistence is what surfaces the
/// `MISSING_HOISTED_LOCATIONS` error during rebuild.
///
/// `skipped` is the depPath list of skipped snapshots: each
/// [`PackageKey`] in the install-time
/// [`crate::SkippedSnapshots`] becomes one string entry; ordering is
/// handled by [`write_modules_manifest`]'s sort-on-write. An empty set
/// produces an empty list — matching the fresh-install case.
///
/// [`PackageKey`]: pacquet_lockfile::PackageKey
/// [`write_modules_manifest`]: pacquet_modules_yaml::write_modules_manifest
#[expect(
    clippy::too_many_arguments,
    reason = "assembles every field of the .modules.yaml manifest from the install's resolved state"
)]
fn build_modules_manifest(
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    hoisted_dependencies: HoistedDependencies,
    hoisted_locations: BTreeMap<String, Vec<String>>,
    injected_deps: BTreeMap<String, Vec<String>>,
    skipped: &crate::SkippedSnapshots,
    ignored_builds: &[String],
    pending_builds: Vec<String>,
    pruned_at: String,
) -> Modules {
    Modules {
        // The `name@version` keys whose build scripts were blocked, so a
        // later install can re-run any that an `allowBuilds` change now
        // allows (see [`has_newly_allowed_ignored_builds`]). `None` when
        // empty, matching pnpm's omit-when-empty encoding.
        ignored_builds: (!ignored_builds.is_empty()).then(|| {
            ignored_builds.iter().cloned().map(pacquet_modules_yaml::DepPath::from).collect()
        }),
        hoist_pattern: config.hoist_pattern.clone(),
        hoisted_dependencies,
        // `Some(empty)` would round-trip on disk as
        // `hoistedLocations: {}`; the field is unset when empty. Drop it
        // when empty so an isolated install doesn't produce a
        // hoisted-only key.
        hoisted_locations: (!hoisted_locations.is_empty()).then_some(hoisted_locations),
        // Per-source-project virtual-store copies of injected `file:`
        // deps (see [`crate::collect_injected_deps`]). Omitted when
        // empty, matching pnpm's omit-when-empty encoding.
        injected_deps: (!injected_deps.is_empty()).then_some(injected_deps),
        included,
        layout_version: Some(LayoutVersion),
        node_linker: Some(map_node_linker(node_linker)),
        // `${name}@${version}`, where the name is the CLI's published
        // npm name. `pacquet` is an in-repo crate name that never
        // reaches disk, and the crate version is not the release
        // version.
        package_manager: format!("pnpm@{PNPM_VERSION}"),
        pending_builds,
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        // RFC 1123 / `toUTCString()` format. The caller decides whether
        // this is a fresh timestamp (a prune ran or first install) or the
        // preserved prior value.
        pruned_at,
        registries: Some(config.resolved_registries()),
        // `iter_installability` excludes fetch-failure entries so they
        // don't get persisted across installs — optional fetch failures
        // are silently swallowed.
        skipped: skipped.iter_installability().map(ToString::to_string).collect(),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config.effective_virtual_store_dir().to_string_lossy().into_owned(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        // The build-approval set this install ran under. A GVS install
        // hashes engine-specific slots for allowed builders, so the
        // recorded set is what a later install diffs against to decide
        // whether its slots need re-linking.
        allow_builds: Some(
            config
                .allow_builds
                .iter()
                .map(|(spec, allowed)| {
                    (spec.clone(), pacquet_modules_yaml::AllowBuildValue::Bool(*allowed))
                })
                .collect(),
        ),
        virtual_store_only: config.virtual_store_only.then_some(true),
        ..Default::default()
    }
}

/// Drop `settled` from the `pendingBuilds` the install just wrote, now
/// that the projects' scripts have run.
///
/// A project's debt outlives the `.modules.yaml` write — its scripts run
/// after it — so clearing the record there would forget the debt when a
/// script fails. Re-reading rather than reusing the in-memory value
/// keeps every other field exactly as it was written.
fn drain_settled_projects<Sys>(modules_dir: &Path, settled: &[String]) -> Result<(), InstallError>
where
    Sys: pacquet_modules_yaml::FsReadToString
        + pacquet_modules_yaml::Clock
        + pacquet_modules_yaml::FsCreateDirAll
        + pacquet_modules_yaml::FsWrite,
{
    if settled.is_empty() {
        return Ok(());
    }
    let Some(mut modules) = pacquet_modules_yaml::read_modules_manifest::<Sys>(modules_dir)
        .map_err(InstallError::ReadModules)?
    else {
        return Ok(());
    };
    let before = modules.pending_builds.len();
    modules.pending_builds.retain(|entry| !settled.contains(entry));
    if modules.pending_builds.len() == before {
        return Ok(());
    }
    write_modules_manifest::<Sys>(modules_dir, modules).map_err(InstallError::WriteModules)
}

/// Includes the executor's implicit `node-gyp rebuild` fallback when a
/// project has `binding.gyp` but no explicit preinstall or install script.
fn project_requires_lifecycle_scripts(project_dir: &Path, manifest: &PackageManifest) -> bool {
    let has_lifecycle_script = pacquet_executor::PROJECT_LIFECYCLE_STAGES
        .iter()
        .any(|stage| matches!(manifest.script(stage, true), Ok(Some(_))));
    has_lifecycle_script
        || (matches!(manifest.script("preinstall", true), Ok(None))
            && matches!(manifest.script("install", true), Ok(None))
            && project_dir.join("binding.gyp").exists())
}

/// The `pendingBuilds` list for this install: the builds still owed,
/// carried-over entries first, then the ones this install deferred.
///
/// A build stays owed until something runs it, so a carried-over entry
/// survives unless its subject left the current lockfile or this run is
/// the `pnpm rebuild` that discharged it.
fn merge_pending_builds<Deferred>(
    previous: &[String],
    deferred: Deferred,
    current: Option<&Lockfile>,
    rebuild: Option<&crate::RebuildOptions>,
    rebuild_build_policy: Option<&crate::AllowBuildPolicy>,
) -> Vec<String>
where
    Deferred: IntoIterator<Item = String>,
{
    // An importer id and a dep path are both plain strings on disk, so
    // the current lockfile's `importers` — not the shape of the string —
    // decides which one an entry is.
    //
    // Only dependencies are settled here: the build phase has already
    // run by the time this file is written, while a project's scripts
    // run after it. `drain_settled_projects` discharges those once they
    // have actually succeeded. A dependency is settled only when the
    // rebuild both selected it and was allowed to build it — a selected
    // package the policy still blocks stays owed, matching pnpm's "drop
    // only what was actually rebuilt".
    let settled = |entry: &str| {
        let (Some(rebuild), Some(policy)) = (rebuild, rebuild_build_policy) else { return false };
        !current.is_some_and(|current| current.importers.contains_key(entry))
            && rebuild.settles_dependency(entry)
            && policy.check(pacquet_deps_path::remove_suffix(entry)) == Some(true)
    };
    let retained = previous.iter().filter(|entry| {
        current.is_some_and(|current| current_contains_dep_path(current, entry)) && !settled(entry)
    });
    let mut seen = HashSet::new();
    retained.cloned().chain(deferred).filter(|entry| seen.insert(entry.clone())).collect()
}

fn merge_filtered_modules_metadata(
    next: &mut Modules,
    previous: &Modules,
    current: &Lockfile,
    selected: &Lockfile,
) {
    for (dep_path, aliases) in &previous.hoisted_dependencies {
        if !retained_only_dep_path(current, selected, dep_path) {
            continue;
        }
        let retained_aliases = next.hoisted_dependencies.entry(dep_path.clone()).or_default();
        for (alias, kind) in aliases {
            retained_aliases.entry(alias.clone()).or_insert(*kind);
        }
    }
    if let Some(previous_locations) = previous.hoisted_locations.as_ref() {
        for (dep_path, locations) in previous_locations {
            if !retained_only_dep_path(current, selected, dep_path) {
                continue;
            }
            let retained_locations = next.hoisted_locations.get_or_insert_default();
            let retained = retained_locations.entry(dep_path.clone()).or_default();
            for location in locations {
                if !retained.contains(location) {
                    retained.push(location.clone());
                }
            }
        }
    }
    let new_pending_builds = std::mem::take(&mut next.pending_builds);
    for dep_path in &previous.pending_builds {
        if retained_only_dep_path(current, selected, dep_path)
            && !next.pending_builds.contains(dep_path)
        {
            next.pending_builds.push(dep_path.clone());
        }
    }
    for dep_path in new_pending_builds {
        if !next.pending_builds.contains(&dep_path) {
            next.pending_builds.push(dep_path);
        }
    }
    let new_ignored_builds = next.ignored_builds.take();
    if let Some(previous_ignored) = previous.ignored_builds.as_ref() {
        for dep_path in previous_ignored {
            if retained_only_dep_path(current, selected, dep_path.as_str()) {
                let retained_ignored = next.ignored_builds.get_or_insert_default();
                retained_ignored.insert(dep_path.clone());
            }
        }
    }
    if let Some(new_ignored_builds) = new_ignored_builds
        && !new_ignored_builds.is_empty()
    {
        next.ignored_builds.get_or_insert_default().extend(new_ignored_builds);
    }
    let new_skipped = std::mem::take(&mut next.skipped);
    for dep_path in &previous.skipped {
        if retained_only_dep_path(current, selected, dep_path) && !next.skipped.contains(dep_path) {
            next.skipped.push(dep_path.clone());
        }
    }
    for dep_path in new_skipped {
        if !next.skipped.contains(&dep_path) {
            next.skipped.push(dep_path);
        }
    }
    // A source the selected install re-materialized has its targets
    // recomputed in `next`, so the previous file's targets for it are
    // stale — a bumped injected dep moves to a new virtual-store slot and
    // the old one is gone. Only sources no selected importer touched carry
    // their previous targets forward.
    let current_injected_sources = injected_source_paths(current);
    let selected_injected_sources = injected_source_paths(selected);
    if let Some(previous_injected) = previous.injected_deps.as_ref() {
        for (source, targets) in previous_injected {
            if current_injected_sources.contains(source)
                && !selected_injected_sources.contains(source)
            {
                let retained_injected = next.injected_deps.get_or_insert_default();
                retained_injected.entry(source.clone()).or_insert_with(|| targets.clone());
            }
        }
    }
}

fn retained_only_dep_path(current: &Lockfile, selected: &Lockfile, dep_path: &str) -> bool {
    current_contains_dep_path(current, dep_path) && !current_contains_dep_path(selected, dep_path)
}

fn injected_source_paths(lockfile: &Lockfile) -> HashSet<String> {
    lockfile
        .snapshots
        .iter()
        .flat_map(|snapshots| snapshots.keys())
        .chain(lockfile.packages.iter().flat_map(|packages| packages.keys()))
        .filter_map(|key| match key.suffix.version() {
            VersionPart::File(path) => Some(path.strip_prefix("./").unwrap_or(path).to_string()),
            VersionPart::Semver(_) | VersionPart::NonSemver(_) => None,
        })
        .collect()
}

fn current_contains_dep_path(current: &Lockfile, dep_path: &str) -> bool {
    if current.importers.contains_key(dep_path) {
        return true;
    }
    let Ok(key) = dep_path.parse::<pacquet_lockfile::PackageKey>() else { return false };
    current.snapshots.as_ref().is_some_and(|snapshots| snapshots.contains_key(&key))
        || current
            .packages
            .as_ref()
            .is_some_and(|packages| packages.contains_key(&key.without_peer()))
}

/// Read a string field off a project manifest, returning `None` when
/// the field is missing or not a JSON string. Pnpm tolerates either
/// shape — `name`/`version` are advisory metadata in this context, so
/// pacquet matches by silently dropping non-string values.
fn manifest_string_field(manifest: &PackageManifest, key: &str) -> Option<String> {
    manifest.value().get(key).and_then(|v| v.as_str()).map(ToString::to_string)
}

/// Walk every workspace project's `package.json`. Returns `Ok(None)`
/// when no `pnpm-workspace.yaml` exists in (or above) `workspace_root`
/// — the install isn't a workspace install, so the caller should use
/// the top-level `Install.manifest` as its only importer and pass
/// `None` for the `workspace:`-spec lookup.
///
/// One walk feeds both [`build_workspace_packages_map`] (the npm
/// resolver's `workspace:` lookup) and the per-importer manifest list
/// the fresh-resolve path iterates over, so the manifests are read
/// from disk exactly once.
fn load_workspace_projects(
    workspace_root: &std::path::Path,
    workspace_manifest: Option<&pacquet_workspace::WorkspaceManifest>,
) -> Result<Option<Vec<pacquet_workspace::Project>>, pacquet_workspace::FindWorkspaceProjectsError>
{
    let Some(manifest) = workspace_manifest else { return Ok(None) };
    let opts = pacquet_workspace::FindWorkspaceProjectsOpts {
        patterns: Some(pacquet_workspace::workspace_package_patterns(manifest)),
    };
    pacquet_workspace::find_workspace_projects(workspace_root, &opts).map(Some)
}

fn order_project_lifecycle_groups<'a>(
    projects: &[(PathBuf, &'a PackageManifest)],
    ordered_groups: Option<&[Vec<PathBuf>]>,
    workspace_root: &Path,
    lockfile: Option<&Lockfile>,
) -> Result<Vec<Vec<(PathBuf, &'a PackageManifest)>>, InstallError> {
    let normalized_project_dirs = projects
        .iter()
        .map(|(project_dir, _)| pacquet_fs::lexical_normalize(project_dir))
        .collect::<Vec<_>>();
    let grouped_dirs = ordered_groups.map(|groups| {
        groups
            .iter()
            .flatten()
            .map(|project_dir| pacquet_fs::lexical_normalize(project_dir))
            .collect::<HashSet<_>>()
    });
    let explicit_groups_cover_projects = grouped_dirs.as_ref().is_some_and(|grouped_dirs| {
        normalized_project_dirs.iter().all(|project_dir| grouped_dirs.contains(project_dir))
    });
    let lockfile_groups;
    let fallback_groups;
    let ordered_groups = if explicit_groups_cover_projects {
        ordered_groups.expect("checked as present")
    } else if let Some(lockfile) = lockfile {
        let included = normalized_project_dirs.clone();
        let included_set = included.iter().cloned().collect::<HashSet<_>>();
        let graph = projects
            .iter()
            .zip(&normalized_project_dirs)
            .map(|((project_dir, _), normalized_project_dir)| {
                let importer_id =
                    pacquet_workspace::importer_id_from_root_dir(workspace_root, project_dir);
                let dependencies = lockfile
                    .importers
                    .get(&importer_id)
                    .into_iter()
                    .flat_map(|snapshot| {
                        [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]
                            .into_iter()
                            .filter_map(|group| snapshot.get_map_by_group(group))
                            .flat_map(|dependencies| dependencies.values())
                    })
                    .filter_map(|dependency| match &dependency.version {
                        pacquet_lockfile::ImporterDepVersion::Link(target) => {
                            Some(pacquet_fs::lexical_normalize(&project_dir.join(target)))
                        }
                        _ => None,
                    })
                    .filter(|target| included_set.contains(target))
                    .collect();
                (normalized_project_dir.clone(), dependencies)
            })
            .collect();
        lockfile_groups = crate::graph_sequencer(&graph, &included).chunks;
        &lockfile_groups
    } else if ordered_groups.is_some() {
        return Err(InstallError::ProjectLifecycleOrder {
            projects: normalized_project_dirs
                .iter()
                .filter(|project_dir| {
                    !grouped_dirs
                        .as_ref()
                        .expect("ordered groups are present")
                        .contains(*project_dir)
                })
                .map(|project_dir| project_dir.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        });
    } else {
        fallback_groups = normalized_project_dirs
            .iter()
            .cloned()
            .map(|project_dir| vec![project_dir])
            .collect::<Vec<_>>();
        &fallback_groups
    };
    let projects_by_dir = projects
        .iter()
        .map(|project| (pacquet_fs::lexical_normalize(&project.0), project))
        .collect::<HashMap<_, _>>();
    let mut included = HashSet::with_capacity(projects.len());
    let groups = ordered_groups
        .iter()
        .filter_map(|dirs| {
            let group = dirs
                .iter()
                .filter_map(|dir| {
                    projects_by_dir.get(&pacquet_fs::lexical_normalize(dir)).map(|project| {
                        included.insert(project.0.clone());
                        (*project).clone()
                    })
                })
                .collect::<Vec<_>>();
            (!group.is_empty()).then_some(group)
        })
        .collect::<Vec<_>>();
    let missing_projects = projects
        .iter()
        .filter(|(project_dir, _)| !included.contains(project_dir))
        .map(|(project_dir, _)| project_dir.display().to_string())
        .collect::<Vec<_>>();
    if !missing_projects.is_empty() {
        return Err(InstallError::ProjectLifecycleOrder { projects: missing_projects.join(", ") });
    }
    Ok(groups
        .into_iter()
        .filter_map(|group| {
            let group = group
                .into_iter()
                .filter(|(project_dir, manifest)| {
                    project_requires_lifecycle_scripts(project_dir, manifest)
                })
                .collect::<Vec<_>>();
            (!group.is_empty()).then_some(group)
        })
        .collect())
}

/// Run workspace projects' own lifecycle scripts in topological build
/// groups. Projects within one group run concurrently; each group settles
/// before the next starts. Every project re-links its bins immediately
/// before its scripts, after dependency projects' scripts from earlier
/// groups have had a chance to create new bin files.
fn run_projects_lifecycle_scripts<Reporter: self::Reporter>(
    project_groups: &[Vec<(PathBuf, &PackageManifest)>],
    config: &Config,
    node_linker: NodeLinker,
    workspace_root: &Path,
) -> Result<(), InstallError> {
    let modules_dir_basename =
        config.modules_dir.file_name().unwrap_or_else(|| std::ffi::OsStr::new("node_modules"));
    // Same tri-state mapping the dependency-build path applies; see
    // the doc on [`pacquet_config::ScriptsPrependNodePath`].
    let scripts_prepend_node_path = match config.scripts_prepend_node_path {
        pacquet_config::ScriptsPrependNodePath::Always => ExecScriptsPrependNodePath::Always,
        pacquet_config::ScriptsPrependNodePath::Never => ExecScriptsPrependNodePath::Never,
        pacquet_config::ScriptsPrependNodePath::WarnOnly => ExecScriptsPrependNodePath::WarnOnly,
    };
    let mut extra_env = config.extra_env.clone();
    if let Some(node_options) = &config.node_options {
        extra_env.insert("NODE_OPTIONS".to_string(), node_options.clone());
    }
    if config.node_experimental_package_map && !matches!(node_linker, NodeLinker::Pnp) {
        let package_map_path = config.modules_dir.join(crate::package_map::PACKAGE_MAP_FILENAME);
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            crate::make_node_package_map_option(&package_map_path, node_options),
        );
    }
    let max_group_size = project_groups.iter().map(Vec::len).max().unwrap_or(0);
    let extra_node_paths = crate::shim_extra_node_paths(config, node_linker);
    let run_project =
        |(project_dir, manifest): &(PathBuf, &PackageManifest)| -> Result<(), InstallError> {
            let root_modules_dir = project_dir.join(modules_dir_basename);
            let mut direct_dep_names = Vec::new();
            let mut seen = HashSet::new();
            for (name, _) in manifest.dependencies([
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ]) {
                if seen.insert(name) {
                    direct_dep_names.push(name.to_string());
                }
            }
            link_project_bins(&root_modules_dir, &direct_dep_names, &extra_node_paths)
                .map_err(InstallError::ProjectBinLink)?;
            let dep_path = project_dir.to_string_lossy();
            run_project_lifecycle_scripts::<Reporter>(&RunPostinstallHooks {
                dep_path: &dep_path,
                pkg_root: project_dir,
                root_modules_dir: &root_modules_dir,
                init_cwd: workspace_root,
                extra_bin_paths: &config.extra_bin_paths,
                extra_env: &extra_env,
                node_execpath: None,
                npm_execpath: None,
                node_gyp_path: None,
                user_agent: None,
                unsafe_perm: config.unsafe_perm,
                node_gyp_bin: None,
                scripts_prepend_node_path,
                script_shell: None,
                optional: false,
            })
            .map_err(InstallError::ProjectLifecycleScript)?;
            Ok(())
        };
    if max_group_size <= 1 {
        for group in project_groups {
            for project in group {
                run_project(project)?;
            }
        }
        return Ok(());
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(crate::script_thread_count(config.child_concurrency, max_group_size))
        .build()
        .map_err(InstallError::ProjectLifecycleThreadPool)?;
    for group in project_groups {
        pool.install(|| group.par_iter().try_for_each(run_project))?;
    }
    Ok(())
}

/// Inputs for [`install_already_up_to_date`].
pub struct UpToDateFastPathCheck<'a> {
    pub config: &'a Config,
    pub manifest: &'a PackageManifest,
    pub dependency_groups: Vec<DependencyGroup>,
    pub node_linker: NodeLinker,
    /// The CLI-merged effective `supportedArchitectures` (yaml plus
    /// `--cpu` / `--os` / `--libc`) — the fast path must not report
    /// "Already up to date" when a flag changed the target platforms.
    pub supported_architectures: Option<pacquet_package_is_installable::SupportedArchitectures>,
}

/// Pre-runtime twin of the repeat-install short-circuit inside
/// [`Install::run`]: same workspace discovery, same
/// [`check_optimistic_repeat_install`] inputs, callable from a
/// synchronous context so the CLI can finish an up-to-date install
/// before paying for the async runtime, the HTTP client, and the
/// state setup. Returns the workspace root — the reporter `prefix`
/// for the "Already up to date" emission — when the install can
/// short-circuit.
///
/// Failures deliberately collapse to `None`: the caller falls through
/// to the full install path, which reproduces the failure with its
/// established error shape.
#[must_use]
pub fn install_already_up_to_date(check: &UpToDateFastPathCheck<'_>) -> Option<PathBuf> {
    let UpToDateFastPathCheck {
        config,
        manifest,
        dependency_groups,
        node_linker,
        supported_architectures,
    } = check;
    let included = IncludedDependencies {
        dependencies: dependency_groups.contains(&DependencyGroup::Prod),
        dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
        optional_dependencies: dependency_groups.contains(&DependencyGroup::Optional),
    };
    let manifest_dir = manifest.path().parent()?;
    let workspace_dir_opt = configured_or_discovered_workspace_dir(config, manifest_dir).ok()?;
    let workspace_root = workspace_dir_opt.clone().unwrap_or_else(|| manifest_dir.to_path_buf());
    let workspace_manifest = match workspace_dir_opt.as_deref() {
        Some(dir) => pacquet_workspace::read_workspace_manifest(dir).ok()?,
        None => None,
    };
    let catalogs = match config.catalogs.clone() {
        Some(catalogs) => catalogs,
        None => get_catalogs_from_workspace_manifest(workspace_manifest.as_ref()).ok()?,
    };
    let workspace_projects =
        load_workspace_projects(&workspace_root, workspace_manifest.as_ref()).ok()?;
    let project_manifests =
        build_project_manifests_list(&workspace_root, manifest, workspace_projects.as_deref());
    // Match the install pipeline's lockfile source: shared workspaces
    // read the root lockfile, while per-project workspaces read the
    // active project's lockfile.
    let lockfile = if config.lockfile {
        LazyLockfile::deferred(if config.shared_workspace_lockfile {
            workspace_root.clone()
        } else {
            manifest_dir.to_path_buf()
        })
    } else {
        LazyLockfile::disabled()
    };
    // Under `strictDepBuilds`, a recorded-and-still-unapproved ignored
    // build must keep the install failing — never let the pre-runtime
    // fast path report up-to-date and exit 0. Returning `None` falls
    // through to the full `Install::run`, whose optimistic branch raises
    // `ERR_PNPM_IGNORED_BUILDS`. A corrupt / unreadable `.modules.yaml`
    // is treated conservatively the same way (its `Err` can't prove the
    // absence of recorded ignored builds).
    if config.strict_dep_builds {
        match pacquet_modules_yaml::read_modules_layout::<Host>(&config.modules_dir) {
            Ok(Some(modules)) => match unapproved_recorded_ignored_builds(&modules, config) {
                Ok(Some(_)) => return None,
                Ok(None) => {}
                // Unreadable state or a malformed `allowBuilds`: force the
                // full install rather than reporting up-to-date.
                Err(_) => return None,
            },
            Ok(None) => {}
            Err(_) => return None,
        }
    }
    let up_to_date = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: &workspace_root,
        config,
        node_linker: *node_linker,
        included,
        supported_architectures: supported_architectures.as_ref(),
        project_manifests: &project_manifests,
        is_workspace_install: workspace_manifest.is_some(),
        lockfile: MaybeLazyLockfile::Lazy(&lockfile),
        catalogs: &catalogs,
    }) == OptimisticRepeatInstallDecision::UpToDate;
    if !up_to_date {
        return None;
    }
    if gvs_build_markers_may_require_recovery(config) {
        let wanted = lockfile.get().ok().flatten()?;
        if gvs_build_marker_present(wanted, config) {
            return None;
        }
    }
    Some(workspace_root)
}

/// Discovery twin of [`install_already_up_to_date`] for the
/// verify-deps-before-run gate: assemble the same
/// [`OptimisticRepeatInstallCheck`] inputs from a bare directory and run
/// [`crate::check_deps_status_before_run`].
///
/// Returns `None` when `dir` has no manifest and no enclosing workspace:
/// the run/exec command is about to fail with its own missing-manifest
/// error, and reporting "outdated" would only trigger an install that
/// can do nothing but crash with `NO_PKG_MANIFEST` (the same guard
/// pnpm's `checkDepsStatus` applies when it finds neither a root
/// manifest nor a workspace).
///
/// Any other discovery failure conservatively reports the dependencies
/// as unverifiable ("Cannot check whether dependencies are outdated"),
/// matching pnpm's catch-all: in the worst case the configured action
/// runs a redundant install.
#[must_use]
pub fn check_deps_status_before_run_at(
    dir: &Path,
    config: &Config,
) -> Option<crate::RunDepsStatus> {
    let cannot_check = || {
        Some(crate::RunDepsStatus::Outdated {
            issue: "Cannot check whether dependencies are outdated".to_string(),
            install_args: Vec::new(),
        })
    };
    let Ok(workspace_dir_opt) = configured_or_discovered_workspace_dir(config, dir) else {
        return cannot_check();
    };
    let workspace_root = workspace_dir_opt.clone().unwrap_or_else(|| dir.to_path_buf());
    let root_manifest = match pacquet_workspace::read_project_manifest_only(&workspace_root) {
        Ok(manifest) => manifest,
        Err(pacquet_workspace::ReadProjectManifestOnlyError::NoImporterManifestFound {
            ..
        }) if workspace_dir_opt.is_none() => {
            return None;
        }
        Err(_) => return cannot_check(),
    };
    let workspace_manifest = match workspace_dir_opt.as_deref() {
        Some(dir) => match pacquet_workspace::read_workspace_manifest(dir) {
            Ok(manifest) => manifest,
            Err(_) => return cannot_check(),
        },
        None => None,
    };
    // pnpm reports "cannot check" straight from the missing workspace
    // state, before any project discovery — a fresh project (the common
    // out-of-sync case) must not pay for the workspace-projects walk
    // only to reach the same verdict inside the check.
    let Ok(Some(workspace_state)) = pacquet_workspace_state::load_workspace_state(&workspace_root)
    else {
        return cannot_check();
    };
    let catalogs = match config.catalogs.clone() {
        Some(catalogs) => catalogs,
        None => match get_catalogs_from_workspace_manifest(workspace_manifest.as_ref()) {
            Ok(catalogs) => catalogs,
            Err(_) => return cannot_check(),
        },
    };
    let Ok(workspace_projects) =
        load_workspace_projects(&workspace_root, workspace_manifest.as_ref())
    else {
        return cannot_check();
    };
    let project_manifests = build_project_manifests_list(
        &workspace_root,
        &root_manifest,
        workspace_projects.as_deref(),
    );
    let lockfile = if config.lockfile {
        LazyLockfile::deferred(workspace_root.clone())
    } else {
        LazyLockfile::disabled()
    };
    Some(crate::check_deps_status_before_run(
        &OptimisticRepeatInstallCheck {
            workspace_root: &workspace_root,
            config,
            node_linker: config.node_linker,
            supported_architectures: config.supported_architectures.as_ref(),
            // The gate ignores dependency-group drift, so the groups only
            // shape the settings snapshot written back after a passing
            // content check — where the recorded values win anyway.
            included: IncludedDependencies {
                dependencies: true,
                dev_dependencies: true,
                optional_dependencies: true,
            },
            project_manifests: &project_manifests,
            is_workspace_install: workspace_manifest.is_some(),
            lockfile: MaybeLazyLockfile::Lazy(&lockfile),
            catalogs: &catalogs,
        },
        &workspace_state,
    ))
}

fn build_project_manifests_list<'a>(
    workspace_root: &std::path::Path,
    root_manifest: &'a PackageManifest,
    workspace_projects: Option<&'a [pacquet_workspace::Project]>,
) -> Vec<(std::path::PathBuf, &'a PackageManifest)> {
    let Some(projects) = workspace_projects else {
        return vec![(workspace_root.to_path_buf(), root_manifest)];
    };
    let active_dir = root_manifest.path().parent().expect("manifest path always has a parent dir");
    let active_dir_matcher = ProjectDirMatcher::new(active_dir);
    let mut active_project_was_discovered = false;
    let mut list = projects
        .iter()
        .map(|project| {
            if active_dir_matcher.matches(&project.root_dir) {
                active_project_was_discovered = true;
                (active_dir.to_path_buf(), root_manifest)
            } else {
                (project.root_dir.clone(), &project.manifest)
            }
        })
        .collect::<Vec<_>>();
    let active_manifest_has_dependencies = root_manifest
        .dependencies([
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
            DependencyGroup::Peer,
        ])
        .next()
        .is_some();
    if !active_project_was_discovered
        && (root_manifest.path().is_file() || active_manifest_has_dependencies)
    {
        list.push((active_dir.to_path_buf(), root_manifest));
    }
    list
}

fn build_root_importer_project_manifests_list<'a>(
    workspace_root: &Path,
    root_manifest: &'a PackageManifest,
    workspace_projects: Option<&'a [pacquet_workspace::Project]>,
) -> Vec<(PathBuf, &'a PackageManifest)> {
    let mut list = vec![(workspace_root.to_path_buf(), root_manifest)];
    if let Some(projects) = workspace_projects {
        let workspace_root_matcher = ProjectDirMatcher::new(workspace_root);
        list.extend(
            projects
                .iter()
                .filter(|project| !workspace_root_matcher.matches(&project.root_dir))
                .map(|project| (project.root_dir.clone(), &project.manifest)),
        );
    }
    list
}

fn build_selected_project_manifests_list<'a>(
    active_manifest: &'a PackageManifest,
    projects: &'a [pacquet_workspace::Project],
    active_manifest_is_standin: bool,
) -> Vec<(PathBuf, &'a PackageManifest)> {
    let mut manifests = projects
        .iter()
        .map(|project| (project.root_dir.clone(), &project.manifest))
        .collect::<Vec<_>>();
    let active_dir =
        active_manifest.path().parent().expect("manifest path always has a parent dir");
    let active_dir_matcher = ProjectDirMatcher::new(active_dir);
    let active_project_was_discovered =
        projects.iter().any(|project| active_dir_matcher.matches(&project.root_dir));
    if !active_manifest_is_standin && !active_project_was_discovered {
        manifests.push((active_dir.to_path_buf(), active_manifest));
    }
    manifests
}

/// Matches workspace project roots against one fixed directory.
///
/// Two paths can name the same project without being equal as strings —
/// a symlinked workspace root, or `/tmp` against its `/private/tmp` target
/// on macOS — so a lexical mismatch falls back to comparing canonical
/// paths. Canonicalizing touches the filesystem once per candidate, so the
/// fixed side is resolved up front rather than once per project.
struct ProjectDirMatcher {
    normalized: PathBuf,
    canonical: Option<PathBuf>,
}

impl ProjectDirMatcher {
    fn new(dir: &Path) -> Self {
        let normalized = pacquet_fs::lexical_normalize(dir);
        let canonical = std::fs::canonicalize(&normalized).ok();
        ProjectDirMatcher { normalized, canonical }
    }

    fn matches(&self, project_dir: &Path) -> bool {
        let project_dir = pacquet_fs::lexical_normalize(project_dir);
        if project_dir == self.normalized {
            return true;
        }
        let Some(canonical) = self.canonical.as_deref() else {
            return false;
        };
        std::fs::canonicalize(project_dir).is_ok_and(|project_dir| project_dir == canonical)
    }
}

fn selected_manifest_freshness_inputs<'a>(
    workspace_root: &Path,
    project_manifests: &[(PathBuf, &'a PackageManifest)],
    selected_dirs: &HashSet<PathBuf>,
) -> Vec<(String, &'a PackageManifest)> {
    let selected_dirs =
        selected_dirs.iter().map(|dir| pacquet_fs::lexical_normalize(dir)).collect::<HashSet<_>>();
    let mut inputs = project_manifests
        .iter()
        .filter(|(project_dir, _)| {
            selected_dirs.contains(&pacquet_fs::lexical_normalize(project_dir))
        })
        .map(|(project_dir, manifest)| {
            (pacquet_workspace::importer_id_from_root_dir(workspace_root, project_dir), *manifest)
        })
        .collect::<Vec<_>>();
    inputs.sort_by(|(left, _), (right, _)| left.cmp(right));
    inputs
}

fn configured_or_discovered_workspace_dir(
    config: &Config,
    manifest_dir: &Path,
) -> Result<Option<PathBuf>, pacquet_workspace::FindWorkspaceDirError> {
    match config.workspace_dir.clone() {
        Some(workspace_dir) => Ok(Some(workspace_dir)),
        None => pacquet_workspace::find_workspace_dir(manifest_dir),
    }
}

/// Build the `name → version → WorkspacePackage` lookup the npm
/// resolver consults for `workspace:` specs. Returns `None` when
/// `projects` is `None` (no workspace) so any `workspace:` spec the
/// manifest happens to carry surfaces
/// [`pacquet_resolving_npm_resolver::ResolveFromWorkspaceError::WorkspacePackagesNotLoaded`].
///
/// The map is a name/version index of per-project `WorkspacePackage`
/// entries (`{ rootDir, manifest }`) consumed by the resolver.
/// Projects whose manifest lacks a name are skipped. A missing or null
/// version is indexed as `0.0.0`; malformed non-string versions are skipped.
fn build_workspace_packages_map(
    projects: Option<&[pacquet_workspace::Project]>,
) -> Option<pacquet_resolving_resolver_base::WorkspacePackages> {
    let projects = projects?;
    let mut map: pacquet_resolving_resolver_base::WorkspacePackages =
        std::collections::BTreeMap::new();
    for project in projects {
        let Some(name) = manifest_string_field(&project.manifest, "name") else { continue };
        let version = match project.manifest.value().get("version") {
            None => "0.0.0".to_string(),
            Some(value) if value.is_null() => "0.0.0".to_string(),
            Some(value) => {
                let Some(version) = value.as_str() else { continue };
                version.to_string()
            }
        };
        map.entry(name).or_default().insert(
            version,
            pacquet_resolving_resolver_base::WorkspacePackage {
                root_dir: project.root_dir.clone(),
                manifest: project.manifest.value().clone(),
            },
        );
    }
    Some(map)
}

pub(crate) fn should_write_package_map(config: &Config, node_linker: NodeLinker) -> bool {
    node_linker == NodeLinker::Isolated && !config.virtual_store_only
}

/// Build the `projects` map for [`WorkspaceState`] from the
/// in-memory `(root_dir, manifest)` list the caller already
/// assembled.
///
/// Pure in-memory: no file I/O, no read-failure warnings, no lockfile
/// or importer traversal. Every project — root and siblings alike —
/// reuses the [`PackageManifest`] reference already loaded for the
/// install dispatch.
fn build_projects_map(
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
) -> BTreeMap<String, ProjectEntry> {
    project_manifests
        .iter()
        .map(|(project_dir, manifest)| {
            let entry = ProjectEntry {
                name: manifest_string_field(manifest, "name"),
                version: manifest_string_field(manifest, "version"),
            };
            (project_dir.to_string_lossy().into_owned(), entry)
        })
        .collect()
}

/// Assemble the [`WorkspaceState`] payload for [`update_workspace_state`].
///
/// Records the projects pacquet just materialized plus the resolved
/// settings the install used.
/// Settings pacquet does not track yet (e.g. `peersSuffixMaxLength`)
/// are omitted; pnpm's `checkDepsStatus`
/// only iterates fields present in the serialized object, so an
/// absent key is silently skipped rather than treated as a drift.
#[expect(
    clippy::too_many_arguments,
    reason = "the workspace-state writer records the install run's resolved inputs"
)]
pub(crate) fn build_workspace_state(
    workspace_root: &Path,
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    supported_architectures: Option<&pacquet_package_is_installable::SupportedArchitectures>,
    catalogs: &Catalogs,
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
    filtered_install: bool,
) -> WorkspaceState {
    WorkspaceState {
        // Record the freshness baseline from the lockfile this install
        // just wrote, not the wall clock. The repeat-install fast path
        // compares this timestamp against file mtimes, so the two must be
        // on the same clock: on a runner whose wall clock runs ahead of
        // the filesystem's mtime clock (observed ~2 ms on some CI
        // microVMs), a `now_millis()` baseline can sit above the mtime of
        // a manifest/pnpmfile edited moments later, hiding the edit and
        // wrongly keeping the fast path. The lockfile's own mtime shares
        // the filesystem clock with every file the check compares, so no
        // skew is possible. Mirrors pnpm's `checkDepsStatus`, whose
        // single-project path keys off the wanted lockfile's mtime. Fall
        // back to the wall clock only when no lockfile was written.
        last_validated_timestamp: crate::optimistic_repeat_install::validation_baseline_ms(
            workspace_root,
            config,
            project_manifests,
        )
        .unwrap_or_else(now_millis),
        projects: build_projects_map(project_manifests),
        pnpmfiles: crate::optimistic_repeat_install::current_pnpmfiles(workspace_root),
        filtered_install,
        config_dependencies: config.config_dependencies.clone(),
        // Settings construction is shared with
        // `optimistic_repeat_install::current_settings` so the
        // freshness check sees the same byte shape this writer
        // produces. Keeping the construction in one place guarantees
        // adding a field on one side doesn't silently flip the other
        // into "drift" on the next install.
        settings: crate::optimistic_repeat_install::current_settings_with_catalogs(
            config,
            node_linker,
            included,
            supported_architectures,
            catalogs,
        ),
    }
}

#[cfg(test)]
mod tests;
