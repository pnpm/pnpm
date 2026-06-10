use crate::{
    BuildVerifiersError, HoistedDependencies, InstallFrozenLockfile, InstallFrozenLockfileError,
    InstallWithFreshLockfile, InstallWithFreshLockfileError, OptimisticRepeatInstallCheck,
    ResolvedPackages, UpdateSeedPolicy, build_resolution_verifiers,
    check_optimistic_repeat_install,
    optimistic_repeat_install::Decision as OptimisticRepeatInstallDecision,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_config::{
    InvalidCatalogsConfigurationError, get_catalogs_from_workspace_manifest,
};
use pacquet_catalogs_types::Catalogs;
use pacquet_config::{Config, NodeLinker};
use pacquet_executor::{
    LifecycleScriptError, RunPostinstallHooks,
    ScriptsPrependNodePath as ExecScriptsPrependNodePath, run_project_lifecycle_scripts,
};
use pacquet_lockfile::{
    LoadLockfileError, Lockfile, SaveLockfileError, StalenessReason, satisfies_package_manifest,
};
use pacquet_lockfile_verification::{
    VerifyError, VerifyLockfileResolutionsOptions, record_lockfile_verified,
    verify_lockfile_resolutions,
};
use pacquet_modules_yaml::{
    Host, IncludedDependencies, LayoutVersion, Modules, NodeLinker as ModulesNodeLinker,
    WriteModulesError, read_modules_manifest, write_modules_manifest,
};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::{
    ContextLog, LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, PnpmLog, Reporter,
    Stage, StageLog, SummaryLog,
};
use pacquet_resolving_npm_resolver::InMemoryPackageMetaCache;
use pacquet_tarball::MemCache;
use pacquet_workspace_state::{
    ProjectEntry, UpdateWorkspaceStateError, WorkspaceState, now_millis, update_workspace_state,
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, atomic::AtomicU8},
    time::SystemTime,
};

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
    pub lockfile: Option<&'a Lockfile>,
    /// Absolute path of the loaded `pnpm-lock.yaml`. Threaded into
    /// the lockfile-verification gate so the per-path stat shortcut
    /// in `<cache_dir>/lockfile-verified.jsonl` can fire on repeat
    /// installs, and into the `pnpm:lockfile-verification` reporter
    /// payload. `None` disables the cache for this run (every call
    /// re-verifies) and falls back to deriving the path from
    /// `workspace_root`. Mirrors upstream's `lockfilePath` argument
    /// at <https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/index.ts#L355-L383>.
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
    /// `--ignore-manifest-check` on the CLI; intended for the pnpm
    /// CLI's `configDependencies` delegation path, where pnpm has
    /// just resolved and written the lockfile but hasn't yet written
    /// the updated manifest. Settings-drift checks (`overrides`,
    /// `ignoredOptionalDependencies`, ...) still run — they don't
    /// inspect the manifest and the bug this flag addresses is
    /// specifically the per-dep specifier mismatch from
    /// <https://github.com/pnpm/pnpm/issues/11797>.
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
    /// CLI — matching pnpm's stance on similar flags. Threaded as a
    /// separate field for the same reason [`Self::skip_runtimes`] is:
    /// `state.config` is a shared `&'static Config`, so the CLI
    /// override merge happens in the caller and lands here as a
    /// fully-resolved value.
    pub trust_lockfile: bool,
    /// Refresh locked integrity values from the registry. Skips the
    /// frozen-lockfile path so the fresh-resolve path rewrites them.
    /// Mirrors pnpm's `--update-checksums`.
    pub update_checksums: bool,
    /// Whether this is a full project install (`pacquet install`,
    /// pnpm's `mutation: 'install'`) rather than a partial one
    /// (`pacquet add`, pnpm's `mutation: 'installSome'`). Gates the
    /// project's own lifecycle scripts: pnpm only runs them for the
    /// full install via the `mutation === 'install'` filter at
    /// <https://github.com/pnpm/pnpm/blob/80037699fb/pkg-manager/core/src/install/index.ts#L1524>,
    /// so a named install such as `pacquet add foo` does not fire
    /// the root project's preinstall/postinstall/prepare/etc.
    pub is_full_install: bool,
    /// `supportedArchitectures` after merging
    /// `Config::supported_architectures` from `pnpm-workspace.yaml`
    /// with the CLI per-axis overrides (`--cpu` / `--os` / `--libc`).
    /// Threaded into `InstallabilityHost` in the frozen-lockfile
    /// path so optional platform-tagged dependencies for the listed
    /// triples are kept even when they don't match the host. `None`
    /// means "host triple is the sole accept set" — same as
    /// upstream's behavior when neither yaml nor CLI sets a value.
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
    /// counterpart — upstream lists `lockfile-only` in `excludedPnpmKeys`),
    /// so it is threaded straight from the CLI like
    /// [`Self::frozen_lockfile`]. Mirrors pnpm's
    /// [`lockfileOnly`](https://github.com/pnpm/pnpm/blob/3b62f9da31/config/reader/src/Config.ts#L170)
    /// (`like npm's --package-lock-only`).
    pub lockfile_only: bool,
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
    /// tarball downloads overlap server-side resolution
    /// ([pnpm/pnpm#12234](https://github.com/pnpm/pnpm/issues/12234)).
    /// Ignored on the frozen path (no tree walk to observe).
    pub resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
}

/// Error type of [`Install`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallError {
    #[display(
        "Headless installation requires a pnpm-lock.yaml file, but none was found. Run `pacquet install` without --frozen-lockfile to create one."
    )]
    #[diagnostic(code(pacquet_package_manager::no_lockfile))]
    NoLockfile,

    #[diagnostic(transparent)]
    WithFreshLockfile(#[error(source)] InstallWithFreshLockfileError),

    /// `--no-runtime` (or `config.skip_runtimes`) is honored only on
    /// the frozen-lockfile path today, where the runtime filter runs
    /// against the loaded lockfile's `packages:` map. A non-frozen
    /// install would still fetch + materialize runtime archives
    /// despite the opt-out, so refuse the install instead of
    /// silently ignoring the flag.
    #[display(
        "--no-runtime / skipRuntimes is not supported without --frozen-lockfile yet. Re-run with --frozen-lockfile against an existing pnpm-lock.yaml, or drop the flag."
    )]
    #[diagnostic(code(pacquet_package_manager::unsupported_fresh_install_skip_runtimes))]
    UnsupportedFreshInstallSkipRuntimes,

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
    WriteModules(#[error(source)] WriteModulesError),

    /// Surfaces a corrupted `<virtual_store_dir>/lock.yaml` rather
    /// than silently skipping the optimization. Mirrors upstream's
    /// `ignoreIncompatible: false` posture at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L226-L227>.
    #[diagnostic(transparent)]
    LoadCurrentLockfile(#[error(source)] LoadLockfileError),

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

    /// `pnpm-lock.yaml` doesn't match the on-disk `package.json` for
    /// the project being installed. Mirrors upstream's
    /// `ERR_PNPM_OUTDATED_LOCKFILE` thrown from
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/pkg-manager/core/src/install/index.ts#L823>:
    /// the user (or CI) edited the manifest without regenerating the
    /// lockfile, and a frozen install would silently produce the
    /// wrong shape of `node_modules`. Fail the install instead.
    #[display(
        "Cannot install with \"frozen-lockfile\" because pnpm-lock.yaml is not up to date with package.json.\n\n  Failure reason:\n  {reason}"
    )]
    #[diagnostic(
        code(pacquet_package_manager::outdated_lockfile),
        help(
            "Regenerate the lockfile with `pnpm install --lockfile-only` so that pnpm-lock.yaml reflects the current package.json, then re-run `pacquet install --frozen-lockfile`."
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
    #[diagnostic(code(pacquet_package_manager::no_importer))]
    NoImporter { importer_id: String },

    /// Mirrors upstream pnpm's `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE`.
    #[display(
        "Cannot use --frozen-lockfile together with --update-checksums: frozen installs never rewrite pnpm-lock.yaml, but --update-checksums exists to do exactly that."
    )]
    #[diagnostic(code(pacquet_package_manager::frozen_lockfile_with_outdated_lockfile))]
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
    /// Mirrors upstream's `INVALID_MINIMUM_RELEASE_AGE_EXCLUDE` /
    /// `INVALID_TRUST_POLICY_EXCLUDE` codes; the inner diagnostic
    /// carries the offending pattern.
    #[diagnostic(transparent)]
    BuildVerifiers(#[error(source)] BuildVerifiersError),

    /// The lockfile-verification gate rejected one or more lockfile
    /// entries — the lockfile contains versions weaker than the
    /// active `minimumReleaseAge` / `trustPolicy='no-downgrade'`
    /// policies allow. Transparent so the inner miette code
    /// (`MINIMUM_RELEASE_AGE_VIOLATION`, `TRUST_DOWNGRADE`,
    /// `LOCKFILE_RESOLUTION_VERIFICATION`) is what the user sees,
    /// matching upstream's `PnpmError` codes byte-for-byte.
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

    /// A value in `pnpm.overrides` couldn't be parsed — the selector
    /// key isn't a recognizable package name, or the override value
    /// uses the `catalog:` protocol (which pacquet doesn't support
    /// yet). Mirrors upstream's `ERR_PNPM_INVALID_SELECTOR` and
    /// `ERR_PNPM_CATALOG_IN_OVERRIDES` codes from
    /// [`config/parse-overrides`](https://github.com/pnpm/pnpm/blob/4a36b9a110/config/parse-overrides/src/index.ts).
    #[diagnostic(transparent)]
    InvalidOverrides(#[error(source)] pacquet_config_parse_overrides::ParseOverridesError),

    /// `--lockfile-only` was requested together with `lockfile: false`
    /// (pnpm's `useLockfile: false`). There is nothing left to do — the
    /// only output `--lockfile-only` produces is the lockfile, and that
    /// write is disabled — so the combination is a user-config conflict
    /// rather than a silent no-op. Mirrors pnpm's
    /// `ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE` thrown
    /// from
    /// <https://github.com/pnpm/pnpm/blob/a33c4bfcb0/installing/deps-installer/src/install/extendInstallOptions.ts#L410-L415>.
    #[display("Cannot generate a pnpm-lock.yaml because lockfile is set to false")]
    #[diagnostic(code(ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE))]
    ConfigConflictLockfileOnlyWithNoLockfile,
}

impl<DependencyGroupList> Install<'_, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    /// Execute the subroutine.
    pub async fn run<Reporter: self::Reporter + 'static>(self) -> Result<(), InstallError> {
        let Install {
            tarball_mem_cache,
            resolved_packages,
            http_client,
            http_client_arc,
            config,
            manifest,
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
            supported_architectures,
            node_linker,
            lockfile_only,
            update_seed_policy,
            auth_override,
            resolution_observer,
        } = self;

        // `--lockfile-only` with `lockfile: false` (pnpm's
        // `useLockfile: false`) is a config conflict: the only output the
        // flag produces is the lockfile, and that write is disabled.
        // Fail fast rather than run a resolve that writes nothing.
        // Mirrors pnpm's `extendInstallOptions` guard.
        if lockfile_only && !config.lockfile {
            return Err(InstallError::ConfigConflictLockfileOnlyWithNoLockfile);
        }

        // Resolve the effective `preferFrozenLockfile` for the
        // dispatch: a per-invocation CLI flag wins over
        // `config.prefer_frozen_lockfile`.
        let prefer_frozen_lockfile =
            prefer_frozen_lockfile.unwrap_or(config.prefer_frozen_lockfile);

        // Collect once so the same set drives both the install dispatch
        // and the `included` field of `.modules.yaml` written below.
        // Mirrors upstream `ctx.include` at
        // <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-installer/src/install/index.ts#L1612>,
        // which is the same set the dependency-graph walker observes.
        let dependency_groups: Vec<DependencyGroup> = dependency_groups.into_iter().collect();
        let included = IncludedDependencies {
            dependencies: dependency_groups.contains(&DependencyGroup::Prod),
            dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
            optional_dependencies: dependency_groups.contains(&DependencyGroup::Optional),
        };

        // Project root for the [bunyan]-envelope `prefix`. Upstream pnpm
        // emits this as `lockfileDir`, the directory containing
        // `pnpm-lock.yaml`. With workspace support that equals the
        // workspace root — pacquet finds it via [`find_workspace_dir`]
        // (port of upstream's `findWorkspaceDir`). Falls back to the
        // manifest's parent dir when no `pnpm-workspace.yaml` exists in
        // any ancestor, matching upstream's single-project behavior.
        // Closes pnpm/pacquet#357.
        //
        // [bunyan]: <https://github.com/trentm/node-bunyan>
        let manifest_dir = manifest.path().parent().expect("manifest path always has a parent dir");
        let workspace_dir_opt = pacquet_workspace::find_workspace_dir(manifest_dir)
            .map_err(InstallError::FindWorkspaceDir)?;
        let workspace_root =
            workspace_dir_opt.clone().unwrap_or_else(|| manifest_dir.to_path_buf());

        // Read `pnpm-workspace.yaml` for the catalog sections. Only
        // consulted when a workspace manifest exists — single-project
        // installs have no `catalog:` to honor. Mirrors upstream's
        // `getCatalogsFromWorkspaceManifest(readWorkspaceManifest(...))`
        // pipeline at
        // <https://github.com/pnpm/pnpm/blob/a8a8cbce6d/catalogs/config/src/getCatalogsFromWorkspaceManifest.ts>.
        let workspace_manifest = match workspace_dir_opt.as_deref() {
            Some(dir) => pacquet_workspace::read_workspace_manifest(dir)
                .map_err(InstallError::ReadWorkspaceManifest)?,
            None => None,
        };
        // Prefer catalogs an `updateConfig` pnpmfile hook produced
        // (`config.catalogs`, the complete set after the hook pass) over
        // the raw workspace-manifest read, mirroring pnpm using the
        // post-`updateConfig` `config.catalogs`. `None` means no hook
        // changed them, so fall back to the manifest.
        let catalogs = match config.catalogs.clone() {
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
        let workspace_projects =
            load_workspace_projects(&workspace_root, workspace_manifest.as_ref())
                .map_err(InstallError::FindWorkspaceProjects)?;

        // Optimistic repeat-install short-circuit. When nothing has
        // changed since the previous successful install (settings,
        // workspace structure, manifest mtimes), skip the entire
        // install pipeline and emit pnpm's "Already up to date" log.
        // Mirrors upstream's
        // [`installDeps`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/installing/commands/src/installDeps.ts#L179-L194)
        // dispatch — the fast path runs before any of the install
        // setup (no lockfile reads, no verifier fan-out, no
        // `getContext`).
        //
        // Disabled when `--frozen-lockfile` is requested: an explicit
        // headless install should always go through the dispatch so a
        // `NoLockfile` or `OutdatedLockfile` error still fires when
        // the lockfile is missing or stale. Mirrors upstream's
        // `installDeps` not calling `checkDepsStatus` when CI mode is
        // forcing the frozen path.
        let project_manifests =
            build_project_manifests_list(&workspace_root, manifest, workspace_projects.as_deref());
        // `pacquet update` must always re-resolve, so it bypasses the
        // optimistic short-circuit: a compatible bump leaves the
        // manifest byte-identical, which the repeat-install check would
        // otherwise read as "nothing changed → already up to date" and
        // skip the registry re-resolution entirely. Gating on
        // `KeepAll` keeps `install` / `add` on the fast path.
        if matches!(update_seed_policy, UpdateSeedPolicy::KeepAll)
            && !frozen_lockfile
            && check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
                workspace_root: &workspace_root,
                config,
                node_linker,
                included,
                project_manifests: &project_manifests,
                is_workspace_install: workspace_manifest.is_some(),
                lockfile,
                catalogs: &catalogs,
            }) == OptimisticRepeatInstallDecision::UpToDate
        {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Info,
                message: "Already up to date".to_string(),
                prefix: prefix.clone(),
            }));
            Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));
            return Ok(());
        }

        // Register the project against the shared store for prune
        // tracking, once per install at the workspace root. Mirrors
        // upstream's call into `@pnpm/store.controller`'s
        // [`registerProject`](https://github.com/pnpm/pnpm/blob/d8a79a9c30/store/controller/src/storeController/projectRegistry.ts)
        // from `getContext` at
        // <https://github.com/pnpm/pnpm/blob/d8a79a9c30/installing/context/src/index.ts#L128>:
        // pnpm registers `opts.lockfileDir` (the workspace root) once,
        // not per importer — store prune walks the workspace's
        // `node_modules/.pnpm/` to find every installed package, so one
        // registry entry per workspace is enough.
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
            // Mirrors pnpm's
            // [`fs.mkdir(opts.storeDir, { recursive: true })`](https://github.com/pnpm/pnpm/blob/d8a79a9c30/installing/context/src/index.ts#L125)
            // call site right before `registerProject`.
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
        // `package.json` body. Mirrors pnpm's per-project emit at
        // <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/context/src/index.ts#L133>:
        // fires before `pnpm:context` so consumers that key off
        // manifest contents have it ready when the install header
        // renders.
        Reporter::emit(&LogEvent::PackageManifest(PackageManifestLog {
            level: LogLevel::Debug,
            message: PackageManifestMessage::Initial {
                prefix: prefix.clone(),
                initial: manifest.value().clone(),
            },
        }));

        // Load the *current* lockfile that records what the previous
        // install actually materialized in `<virtual_store_dir>/lock.yaml`.
        // The frozen-lockfile path diffs each wanted snapshot against
        // this on a per-`PackageKey` basis to decide whether the
        // already-installed slot is still usable. `Ok(None)` on a
        // first install (the file doesn't exist yet). A corrupted /
        // version-incompatible file surfaces as `LoadCurrentLockfile`
        // and fails the install — matching upstream's
        // `ignoreIncompatible: false` posture at the deps-restorer
        // call site rather than silently dropping the optimization.
        //
        // Mirrors upstream's `readCurrentLockfile` call at
        // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L226-L227>.
        let current_lockfile =
            Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir)
                .map_err(InstallError::LoadCurrentLockfile)?;

        // Synthesize the wanted lockfile from `<virtual_store_dir>/lock.yaml`
        // when `pnpm-lock.yaml` is absent and the materialized snapshot still
        // satisfies the manifest. The install then skips resolution and
        // regenerates `pnpm-lock.yaml` from the synthesized object. Mirrors
        // pnpm's `installing/context/src/readLockfiles.ts` clone of
        // `currentLockfile` into the wanted slot at
        // <https://github.com/pnpm/pnpm/blob/8a2146b7be/installing/context/src/readLockfiles.ts#L125-L138>.
        let synthesized_lockfile: Option<Lockfile> =
            if lockfile.is_none() && !frozen_lockfile && prefer_frozen_lockfile {
                current_lockfile.as_ref().and_then(|current| {
                    check_lockfile_freshness(
                        current,
                        manifest,
                        config,
                        &catalogs,
                        ignore_manifest_check,
                    )
                    .ok()
                    .map(|()| current.clone())
                })
            } else {
                None
            };
        let lockfile_synthesized_from_current = synthesized_lockfile.is_some();
        let lockfile = lockfile.or(synthesized_lockfile.as_ref());

        // Lockfile-verification gate: re-apply `minimumReleaseAge` /
        // `trustPolicy='no-downgrade'` to every entry in the loaded
        // `pnpm-lock.yaml` before any resolver or fetcher runs.
        // Mirrors upstream's wiring at
        // <https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/index.ts#L355-L383>.
        // `lockfile.is_none()` (writable-lockfile path) skips the
        // gate entirely — fresh local resolution is already filtered
        // by the resolver's per-version gate (`minimumReleaseAge` via
        // `ResolveResult::policy_violation`, `trustPolicy='no-downgrade'`
        // via the npm resolver's `fail_if_trust_downgraded_for_pick`).
        // `trust_lockfile` (the OR of yaml's
        // `trustLockfile` and the `--trust-lockfile` CLI flag,
        // resolved in [`crate::cli_args::install::InstallArgs::run`])
        // is the opt-out for environments where the install can
        // treat the on-disk lockfile as already-trusted (see [#11860]).
        //
        // [#11860]: <https://github.com/pnpm/pnpm/issues/11860>
        // One per-install packument cache shared with both the
        // lockfile-verifier (below) and the resolver in
        // `install_with_fresh_lockfile` (further down). The
        // single instance lets a name the resolver fetched during this
        // install short-circuit the verifier's own fetch chain, and
        // vice versa. Mirrors pnpm's `installing/client` wiring.
        let meta_cache = Arc::new(InMemoryPackageMetaCache::default());
        let resolution_verifiers = build_resolution_verifiers(
            config,
            Arc::clone(&http_client_arc),
            Some(Arc::clone(&meta_cache)
                as Arc<dyn pacquet_resolving_npm_resolver::PackageMetaCache>),
            auth_override.clone(),
            None,
        )
        .map_err(InstallError::BuildVerifiers)?;

        if let Some(loaded_lockfile) = lockfile.filter(|_| !trust_lockfile) {
            let derived_lockfile_path = lockfile_path
                .map_or_else(|| workspace_root.join(Lockfile::FILE_NAME), Path::to_path_buf);
            verify_lockfile_resolutions::<Reporter>(
                loaded_lockfile,
                &resolution_verifiers,
                &VerifyLockfileResolutionsOptions {
                    concurrency: None,
                    lockfile_path: Some(&derived_lockfile_path),
                    cache_dir: Some(&config.cache_dir),
                },
            )
            .await
            .map_err(InstallError::LockfileVerification)?;
        }

        // `pnpm:context` carries the directories pnpm's reporter prints
        // in the install header. `currentLockfileExists` mirrors
        // upstream's <https://github.com/pnpm/pnpm/blob/94240bc046/installing/context/src/index.ts#L196>:
        // `true` once a previous install has written
        // `<virtual_store_dir>/lock.yaml`.
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
        // emits the channel afresh — mirroring upstream pnpm's per-
        // importer closure capture rather than a process-static.
        let logged_methods = AtomicU8::new(0);

        tracing::info!(target: "pacquet::install", "Start all");

        // Dispatch priority, matching pnpm's CLI + `preferFrozenLockfile`
        // semantics:
        //
        // 1. `--frozen-lockfile` flag → frozen path. Lockfile must exist
        //    and the freshness check (settings + per-importer specifier
        //    match) must pass, otherwise fail. Mirrors upstream's
        //    headless install at
        //    <https://github.com/pnpm/pnpm/blob/94240bc046/pkg-manager/core/src/install/index.ts#L815-L832>.
        //
        // 2. No flag, lockfile present, `prefer_frozen_lockfile == true`,
        //    and the freshness check passes → frozen path (same code as
        //    state 1). Mirrors upstream's
        //    [`preferFrozenLockfile`](https://pnpm.io/settings#preferfrozenlockfile)
        //    fast path: when the lockfile matches the manifest, pnpm
        //    silently goes headless instead of re-resolving against the
        //    registry.
        //
        // 3. No flag, lockfile present, but either `prefer_frozen_lockfile`
        //    is off or the freshness check fails → fresh-resolve path,
        //    seeded from the existing lockfile so unrelated entries keep
        //    their pins. Mirrors upstream's `update: false` resolver mode
        //    at <https://github.com/pnpm/pnpm/blob/097983fbca/lockfile/preferred-versions/src/index.ts#L13-L33>.
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
        let take_frozen_path = if frozen_lockfile {
            let Some(lockfile) = lockfile else {
                return Err(InstallError::NoLockfile);
            };
            // Run the freshness gates; on failure surface a fatal
            // InstallError via `FreshnessCheckError`'s `From` impl.
            // The check is run for its side effect (the typed
            // outcome) — the borrowed lockfile / manifest are consumed
            // again inside the frozen branch below.
            check_lockfile_freshness(lockfile, manifest, config, &catalogs, ignore_manifest_check)
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
                    manifest,
                    config,
                    &catalogs,
                    ignore_manifest_check,
                ) {
                    Ok(()) => true,
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
        // current lockfile, or the workspace-state file. Mirrors pnpm's
        // lockfileOnly short-circuits: the frozen / up-to-date path writes
        // the wanted lockfile and returns at
        // <https://github.com/pnpm/pnpm/blob/a33c4bfcb0/installing/deps-installer/src/install/index.ts#L979-L986>,
        // and the fresh-resolve path skips `linkPackages` at
        // <https://github.com/pnpm/pnpm/blob/a33c4bfcb0/installing/deps-installer/src/install/index.ts#L1543>.
        if lockfile_only && take_frozen_path {
            // Frozen (`--frozen-lockfile`) or auto-frozen
            // (`preferFrozenLockfile`) + `--lockfile-only`: the freshness
            // gate folded into `take_frozen_path` already validated the
            // on-disk lockfile (a stale one surfaced `OutdatedLockfile`).
            // Re-persist it so a brand-new project still lands a file, then
            // return without touching `node_modules`.
            let lockfile = lockfile.expect("frozen dispatch verified lockfile is present");
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
        // spuriously, then exit. Mirrors upstream's `validateModules` +
        // `allProjectsAreUpToDate` fast path at
        // <https://github.com/pnpm/pnpm/blob/a456dc78fb/installing/deps-installer/src/install/index.ts#L913-L985>.
        if take_frozen_path
            && let Some(wanted_lockfile) = lockfile
            && let Some(current) = current_lockfile.as_ref()
            && wanted_lockfile == current
            && is_modules_yaml_consistent(&config.modules_dir, config, node_linker, included)
        {
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
                &build_workspace_state(config, node_linker, included, &project_manifests),
            )
            .map_err(InstallError::WriteWorkspaceState)?;
            Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));
            return Ok(());
        }

        let (hoisted_dependencies, hoisted_locations, frozen_skipped, fresh_lockfile): (
            HoistedDependencies,
            BTreeMap<String, Vec<String>>,
            crate::SkippedSnapshots,
            Option<Lockfile>,
        ) = if take_frozen_path {
            let lockfile = lockfile.expect("dispatch verified lockfile is present");
            let Lockfile { lockfile_version, importers, packages, snapshots, .. } = lockfile;
            assert_eq!(lockfile_version.major, 9); // compatibility check already happens at serde, but this still helps preventing programmer mistakes.

            let frozen_result = InstallFrozenLockfile {
                http_client,
                config,
                importers,
                packages: packages.as_ref(),
                snapshots: snapshots.as_ref(),
                lockfile,
                current_lockfile: current_lockfile.as_ref(),
                current_snapshots: current_lockfile
                    .as_ref()
                    .and_then(|lockfile| lockfile.snapshots.as_ref()),
                current_packages: current_lockfile
                    .as_ref()
                    .and_then(|lockfile| lockfile.packages.as_ref()),
                dependency_groups,
                logged_methods: &logged_methods,
                workspace_root: &workspace_root,
                requester: &prefix,
                supported_architectures: supported_architectures.as_ref(),
                skip_runtimes,
                node_linker,
                tarball_mem_cache: Some(&tarball_mem_cache),
            }
            .run::<Reporter>()
            .await
            .map_err(InstallError::FrozenLockfile)?;

            (
                frozen_result.hoisted_dependencies,
                frozen_result.hoisted_locations,
                frozen_result.skipped,
                None,
            )
        } else {
            // Flag combinations the fresh-lockfile path doesn't honor
            // yet are validated here, after the dispatch decision so an
            // auto-frozen install (state 2 of [`Install::run`]) doesn't
            // get rejected up front:
            //
            // - `skip_runtimes` (CLI `--no-runtime`) on the fresh path
            //   would need a runtime-filter at the materialization step
            //   matching the frozen path's
            //   [`installing/deps-installer/src/install/index.ts:1374-1387`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/index.ts#L1374-L1387)
            //   filter. Without it, runtime archives get fetched +
            //   materialized despite the opt-out.
            //
            // Bypassed under `--lockfile-only`: that path writes only
            // `pnpm-lock.yaml` and never materializes, so the runtime
            // filter is irrelevant to its output. Mirrors pnpm gating its
            // lockfileOnly-specific handling on `!opts.lockfileOnly` at
            // <https://github.com/pnpm/pnpm/blob/a33c4bfcb0/installing/deps-installer/src/install/index.ts#L1957>.
            if !lockfile_only && skip_runtimes {
                return Err(InstallError::UnsupportedFreshInstallSkipRuntimes);
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
            // silently skipping to a registry lookup, matching pnpm's
            // posture at
            // <https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L828-L830>.
            //
            // Reuses the `workspace_projects` walk done at the top of
            // `Install::run` for the optimistic-repeat-install check
            // so we don't pay the workspace scan twice on a
            // fresh-install fall-through.
            let workspace_packages = build_workspace_packages_map(workspace_projects.as_deref());
            // Build the per-importer manifest list. The root importer
            // (`"."`) always reuses the in-memory `Install.manifest`
            // — `pacquet add` mutates that value before calling install,
            // so re-reading from disk would walk the pre-add shape and
            // miss the freshly-added dep. Sibling importers come from
            // the `find_workspace_projects` walk, which read them off
            // disk for `workspace_packages` already.
            let importer_manifests: BTreeMap<String, &PackageManifest> = {
                let mut map = BTreeMap::new();
                map.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), manifest);
                if let Some(projects) = workspace_projects.as_deref() {
                    for project in projects {
                        let id = pacquet_workspace::importer_id_from_root_dir(
                            &workspace_root,
                            &project.root_dir,
                        );
                        if id == Lockfile::ROOT_IMPORTER_KEY {
                            continue;
                        }
                        map.insert(id, &project.manifest);
                    }
                }
                map
            };
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
                catalogs,
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
                // entries keep their pins on rewrite, matching
                // upstream's `update: false` mode. State 4 (no
                // lockfile) passes `None`.
                wanted_lockfile: lockfile,
                node_linker,
                supported_architectures: supported_architectures.as_ref(),
                lockfile_only,
                update_seed_policy,
                auth_override,
                resolution_observer,
            }
            .run::<Reporter>()
            .await
            .map_err(InstallError::WithFreshLockfile)?;

            if fresh_result.can_record_lockfile_verification
                && let Some(lockfile) = fresh_result.wanted_lockfile.as_ref()
            {
                let lockfile_path = workspace_root.join(Lockfile::FILE_NAME);
                record_lockfile_verified(
                    Some(&config.cache_dir),
                    &lockfile_path,
                    lockfile,
                    &resolution_verifiers,
                );
            }

            (
                fresh_result.hoisted_dependencies,
                fresh_result.hoisted_locations,
                crate::SkippedSnapshots::new(),
                fresh_result.wanted_lockfile,
            )
        };

        tracing::info!(target: "pacquet::install", "Complete all");

        // Fresh-resolve `--lockfile-only` already wrote `pnpm-lock.yaml` and
        // emitted `importing_done` inside `InstallWithFreshLockfile::run`.
        // Skip `.modules.yaml`, the current lockfile, and the
        // workspace-state file: there is no `node_modules` to describe, and
        // writing the workspace-state file would make the next install's
        // up-to-date check believe materialization happened. Mirrors pnpm
        // writing only the wanted lockfile at
        // <https://github.com/pnpm/pnpm/blob/a33c4bfcb0/installing/deps-installer/src/install/index.ts#L1784>
        // and skipping `updateWorkspaceState` when `lockfileOnly` at
        // <https://github.com/pnpm/pnpm/blob/a33c4bfcb0/installing/commands/src/installDeps.ts#L515>.
        if lockfile_only {
            Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));
            return Ok(());
        }

        // `Stage::ImportingDone` is emitted inside the install paths
        // (`InstallFrozenLockfile` between symlink and build, and
        // `InstallWithFreshLockfile` after the writer task) so that any
        // subsequent `pnpm:lifecycle` events render after the import
        // progress display has closed. Mirrors upstream's emit point in
        // <https://github.com/pnpm/pnpm/blob/80037699fb/installing/deps-installer/src/install/link.ts#L167>.

        // Write `node_modules/.modules.yaml`. Mirrors upstream's
        // `writeModulesManifest` call at
        // <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-installer/src/install/index.ts#L1608-L1630>,
        // which fires after `importing_done` and before the closing
        // `pnpm:summary` emit. The manifest records the resolved
        // directory layout, hoist patterns, included dependency groups,
        // store dir, and registries so a later install (or another
        // tool) can detect a layout change and prune accordingly.
        write_modules_manifest::<Host>(
            &config.modules_dir,
            build_modules_manifest(
                config,
                node_linker,
                included,
                hoisted_dependencies,
                hoisted_locations,
                &frozen_skipped,
            ),
        )
        .map_err(InstallError::WriteModules)?;

        // Write `<virtual_store_dir>/lock.yaml`. Mirrors upstream's
        // `writeCurrentLockfile` call at
        // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/index.ts#L1597>:
        // captures what was actually materialized so the next install
        // can diff each snapshot against it and skip the unchanged
        // slots. Persist *after* `write_modules_manifest` succeeds so
        // a manifest failure can't leave a fresh current-lockfile
        // pointing at incomplete install state — the next frozen
        // reinstall would otherwise diff against a graph that never
        // finished committing (review on <https://github.com/pnpm/pacquet/pull/442>).
        //
        // Workspace installs (<https://github.com/pnpm/pacquet/issues/431>) ship every importer's section of
        // the wanted lockfile unchanged because the install fans out
        // across all of them. Once `--filter` lands (Stage 2 of
        // <https://github.com/pnpm/pacquet/issues/299>), this needs to narrow to the filtered lockfile
        // (selected importers × engine filter) so the saved current
        // lockfile reflects only what was actually materialized.
        if frozen_lockfile && let Some(lockfile) = lockfile {
            // Filter the wanted lockfile down to the snapshots that
            // were actually materialized: dep maps the user excluded
            // (`--no-optional`, `--no-dev`) plus snapshots the
            // install-time skip set dropped (installability, fetch
            // failure, `--no-optional`-only entries). Ports
            // upstream's
            // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L687-L695>
            // flow — `writeCurrentLockfile(filteredLockfile)`. The
            // next install diffs against this filtered shape so
            // dropped snapshots aren't mistaken for already-done
            // work.
            crate::filter_lockfile_for_current(lockfile, included, &frozen_skipped)
                .save_current_to_virtual_store_dir(&config.virtual_store_dir)
                .map_err(InstallError::SaveCurrentLockfile)?;
        } else if let Some(fresh_lockfile) = fresh_lockfile.as_ref() {
            // Fresh-install path: mirror the frozen behavior by
            // persisting `<virtual_store_dir>/lock.yaml` from the
            // freshly-built wanted lockfile. No filtering needed —
            // the resolver only walked the dep groups the install
            // requested, so the wanted and materialized graphs match
            // by construction. The save is gated on the same
            // `config.lockfile` knob the wanted-side write honors
            // (`fresh_lockfile` is `None` when the opt-out fired).
            fresh_lockfile
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
        // `.modules.yaml` / the current lockfile are written. Mirrors
        // pnpm's `runLifecycleHooksConcurrently(['preinstall', ...])`
        // emit point near the end of the install at
        // <https://github.com/pnpm/pnpm/blob/80037699fb/pkg-manager/core/src/install/index.ts#L1517-L1530>.
        // The `pnpm:lifecycle` events these scripts produce render
        // before the closing `pnpm:summary` below, matching pnpm.
        //
        // Skipped for partial installs (`pacquet add`): pnpm filters
        // to `mutation === 'install'` so a named install does not fire
        // the project's own scripts (see [`Install::is_full_install`]).
        if is_full_install {
            run_projects_lifecycle_scripts::<Reporter>(
                &project_manifests,
                config,
                &workspace_root,
            )?;
        }

        // Write `node_modules/.pnpm-workspace-state-v1.json`. Mirrors
        // upstream's `updateWorkspaceState` call at
        // <https://github.com/pnpm/pnpm/blob/7ff112bac6/installing/commands/src/installDeps.ts#L447-L454>.
        // pnpm's `verifyDepsBeforeRun` gate at
        // <https://github.com/pnpm/pnpm/blob/7ff112bac6/deps/status/src/checkDepsStatus.ts#L80-L86>
        // bails to "outdated" the moment this file is missing,
        // forcing `pnpm install` to rerun. Writing it after both the
        // `.modules.yaml` and the current lockfile succeed mirrors
        // pnpm's ordering and keeps the file pointing at a fully
        // committed install.
        update_workspace_state(
            &workspace_root,
            &build_workspace_state(config, node_linker, included, &project_manifests),
        )
        .map_err(InstallError::WriteWorkspaceState)?;

        // `pnpm:summary` closes the install and lets the reporter render
        // the accumulated `pnpm:root` events as a "+N -M" block. Must
        // come after `importing_done`, matching pnpm's ordering at
        // <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-installer/src/install/index.ts#L1663>.
        Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));

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
/// `ignoredOptionalDependencies`) still runs — see
/// <https://github.com/pnpm/pnpm/issues/11797>.
fn check_lockfile_freshness(
    lockfile: &Lockfile,
    manifest: &PackageManifest,
    config: &Config,
    catalogs: &Catalogs,
    ignore_manifest_check: bool,
) -> Result<(), FreshnessCheckError> {
    let parsed_overrides_opt = parse_config_overrides(config, catalogs)?;
    check_lockfile_settings_drift(lockfile, config, parsed_overrides_opt.as_deref())?;

    if ignore_manifest_check {
        return Ok(());
    }

    // Pacquet has only one importer today (<https://github.com/pnpm/pacquet/issues/431> tracks workspaces),
    // so the root project is the only thing to verify; once
    // workspaces land this becomes a per-project loop over
    // `lockfile.importers`.
    check_importer_satisfies(
        lockfile,
        manifest,
        Lockfile::ROOT_IMPORTER_KEY,
        config,
        parsed_overrides_opt.as_deref(),
    )
}

/// Parse `pnpm.overrides` from the config. Values can use the
/// `catalog:` protocol, which pnpm resolves against the workspace's
/// catalogs *before* writing them to `pnpm-lock.yaml#overrides` —
/// resolving here keeps an override declared as `"foo": "catalog:"`
/// comparable to the lockfile's already-resolved `"foo": "<concrete>"`.
/// Mirrors upstream's
/// <https://github.com/pnpm/pnpm/blob/4a36b9a110/config/parse-overrides/src/index.ts#L20-L44>
/// → `createOverridesMapFromParsed` pipeline.
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
/// check. Mirrors upstream's
/// [`getOutdatedLockfileSetting`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts).
pub(crate) fn check_lockfile_settings_drift(
    lockfile: &Lockfile,
    config: &Config,
    parsed_overrides: Option<&[pacquet_config_parse_overrides::VersionOverride]>,
) -> Result<(), FreshnessCheckError> {
    let overrides_map: Option<std::collections::HashMap<String, String>> =
        parsed_overrides.map(pacquet_config_parse_overrides::create_overrides_map_from_parsed);
    let package_extensions_checksum = config
        .package_extensions
        .as_ref()
        .filter(|extensions| !extensions.is_empty())
        .and_then(|extensions| serde_json::to_value(extensions).ok())
        .as_ref()
        .and_then(pacquet_graph_hasher::hash_object_nullable_with_prefix);
    // `calcPatchHashes(opts.patchedDependencies)` — reading the patch
    // files here lets `check_lockfile_settings` catch an edited patch
    // whose hash (and thus its `(patch_hash=...)` depPath suffix) drifted
    // from what the lockfile recorded.
    let patched_dependency_hashes =
        config.patched_dependency_hashes().map_err(FreshnessCheckError::CalcPatchHashes)?;
    pacquet_lockfile::check_lockfile_settings(
        lockfile,
        overrides_map.as_ref(),
        package_extensions_checksum.as_deref(),
        config.ignored_optional_dependencies.as_deref(),
        patched_dependency_hashes.as_ref(),
        config.inject_workspace_packages,
        config.peers_suffix_max_length,
    )
    .map_err(FreshnessCheckError::Stale)
}

/// Per-importer slice of the freshness gate: the manifest of the
/// project at `importer_id` must still be satisfied by the lockfile's
/// importer snapshot. Mirrors upstream's `satisfiesPackageManifest`
/// call inside
/// [`allProjectsAreUpToDate`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/verification/src/allProjectsAreUpToDate.ts)
/// / `assertWantedLockfileUpToDate`.
pub(crate) fn check_importer_satisfies(
    lockfile: &Lockfile,
    manifest: &PackageManifest,
    importer_id: &str,
    config: &Config,
    parsed_overrides: Option<&[pacquet_config_parse_overrides::VersionOverride]>,
) -> Result<(), FreshnessCheckError> {
    let importer = lockfile
        .importers
        .get(importer_id)
        .ok_or_else(|| FreshnessCheckError::NoImporter { importer_id: importer_id.to_string() })?;

    // Apply `pnpm.overrides` to a *cloned* manifest before the
    // per-importer specifier check so the lockfile's specifiers —
    // written with overrides already applied — match the on-disk
    // manifest's deps. The caller's manifest stays pristine since
    // upstream's read-package-hook conceptually returns a new manifest
    // from the perspective of every consumer downstream of the
    // resolver.
    let overrider_manifest_holder;
    let manifest_for_freshness: &PackageManifest = if let Some(parsed) = parsed_overrides {
        let root_dir = manifest.path().parent().unwrap_or_else(|| Path::new("."));
        let overrider = crate::VersionsOverrider::new(parsed, root_dir);
        overrider_manifest_holder = {
            let mut cloned: PackageManifest = manifest.clone();
            overrider.apply(&mut cloned, Some(root_dir));
            cloned
        };
        &overrider_manifest_holder
    } else {
        manifest
    };

    // Build the `ignoredOptionalDependencies` filter set. Mirrors
    // upstream's
    // [`createOptionalDependenciesRemover`](https://github.com/pnpm/pnpm/blob/94240bc046/hooks/read-package-hook/src/createOptionalDependenciesRemover.ts):
    // the hook iterates `manifest.optionalDependencies` and deletes
    // matches from BOTH the `optional` and `dependencies` maps. A
    // name only present in `dependencies` that happens to match the
    // pattern is NOT removed — set-based ("name was in
    // optionalDependencies AND matched") rather than pure pattern
    // matching. `devDependencies` is untouched on purpose; the group
    // gate inside `satisfies_package_manifest` enforces that.
    let ignored_set: std::collections::HashSet<String> = config
        .ignored_optional_dependencies
        .as_deref()
        .filter(|patterns| !patterns.is_empty())
        .map(|patterns| {
            let matcher = pacquet_config::matcher::create_matcher(patterns);
            manifest_for_freshness
                .dependencies([pacquet_package_manifest::DependencyGroup::Optional])
                .filter(|(name, _)| matcher.matches(name))
                .map(|(name, _)| name.to_string())
                .collect()
        })
        .unwrap_or_default();
    let is_ignored_optional: &dyn Fn(&str) -> bool = &|name: &str| ignored_set.contains(name);

    satisfies_package_manifest(importer, manifest_for_freshness, importer_id, is_ignored_optional)
        .map_err(FreshnessCheckError::Stale)
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
    #[diagnostic(code(pacquet_package_manager::no_importer))]
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
/// matching upstream's `nodeLinker` string.
fn map_node_linker(linker: NodeLinker) -> ModulesNodeLinker {
    match linker {
        NodeLinker::Isolated => ModulesNodeLinker::Isolated,
        NodeLinker::Hoisted => ModulesNodeLinker::Hoisted,
        NodeLinker::Pnp => ModulesNodeLinker::Pnp,
    }
}

/// Check whether `<modules_dir>/.modules.yaml` is present and its
/// recorded layout settings (`nodeLinker`, hoist patterns, store /
/// virtual-store paths, `virtualStoreDirMaxLength`, included dep
/// groups, layout version) match what the current install would
/// produce. Returns `false` when the file is missing, unreadable, or
/// records a different layout — both cases that disqualify the no-op
/// short-circuit.
///
/// Mirrors the settings checks in upstream's
/// [`validateModules`](https://github.com/pnpm/pnpm/blob/a456dc78fb/installing/deps-installer/src/install/validateModules.ts)
/// minus the prune side effects: a settings mismatch in pnpm forces a
/// rewrite of `node_modules`, but pacquet's caller falls through to
/// the regular install path, which rebuilds the layout from scratch
/// anyway.
fn is_modules_yaml_consistent(
    modules_dir: &Path,
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
) -> bool {
    let Some(modules) = read_modules_manifest::<Host>(modules_dir).ok().flatten() else {
        return false;
    };
    modules.layout_version == Some(LayoutVersion)
        && modules.node_linker == Some(map_node_linker(node_linker))
        && modules.included == included
        && modules.hoist_pattern == config.hoist_pattern
        && modules.public_hoist_pattern == config.public_hoist_pattern
        && modules.virtual_store_dir_max_length == config.virtual_store_dir_max_length
        && modules.store_dir == config.store_dir.display().to_string()
        && modules.virtual_store_dir
            == config.effective_virtual_store_dir().to_string_lossy().as_ref()
}

/// Assemble the [`Modules`] payload for [`write_modules_manifest`].
///
/// Mirrors upstream's literal at
/// <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-installer/src/install/index.ts#L1608-L1630>.
/// Fields pacquet does not populate yet (`pendingBuilds`,
/// `injectedDeps`, `ignoredBuilds`, `allowBuilds`) default to empty
/// / unset.
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
/// directory; absent persistence is what surfaces upstream's
/// `MISSING_HOISTED_LOCATIONS` error during rebuild.
///
/// `skipped` is the depPath list pnpm writes at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/index.ts#L1625>:
/// each [`PackageKey`] in the install-time
/// [`crate::SkippedSnapshots`] becomes one string entry; ordering is
/// handled by [`write_modules_manifest`]'s sort-on-write, matching
/// upstream's `saveModules.skipped.sort()`. An empty set produces
/// an empty list — matching the fresh-install case.
///
/// [`PackageKey`]: pacquet_lockfile::PackageKey
/// [`write_modules_manifest`]: pacquet_modules_yaml::write_modules_manifest
fn build_modules_manifest(
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    hoisted_dependencies: HoistedDependencies,
    hoisted_locations: BTreeMap<String, Vec<String>>,
    skipped: &crate::SkippedSnapshots,
) -> Modules {
    Modules {
        hoist_pattern: config.hoist_pattern.clone(),
        hoisted_dependencies,
        // `Some(empty)` would round-trip on disk as
        // `hoistedLocations: {}`, which differs from upstream's
        // unset-when-empty behavior. Drop the field when empty so
        // an isolated install doesn't produce a hoisted-only key.
        hoisted_locations: (!hoisted_locations.is_empty()).then_some(hoisted_locations),
        included,
        layout_version: Some(LayoutVersion),
        node_linker: Some(map_node_linker(node_linker)),
        // `${name}@${version}` per upstream. `CARGO_PKG_VERSION`
        // resolves at compile time to this crate's package version.
        package_manager: concat!("pacquet@", env!("CARGO_PKG_VERSION")).to_string(),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        // RFC 1123 / `toUTCString()` format, matching upstream's
        // `new Date().toUTCString()` at line 1622.
        pruned_at: httpdate::fmt_http_date(SystemTime::now()),
        registries: Some(BTreeMap::from([("default".to_string(), config.registry.clone())])),
        // `iter_installability` excludes fetch-failure entries so they
        // don't get persisted across installs — matches upstream's
        // silent swallow of optional fetch failures at
        // <https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L294-L298>.
        skipped: skipped.iter_installability().map(ToString::to_string).collect(),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config.effective_virtual_store_dir().to_string_lossy().into_owned(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        ..Default::default()
    }
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
/// from disk exactly once. Mirrors upstream's
/// [`findWorkspacePackages`](https://github.com/pnpm/pnpm/blob/3422cecfd3/workspace/find-packages/src/index.ts).
fn load_workspace_projects(
    workspace_root: &std::path::Path,
    workspace_manifest: Option<&pacquet_workspace::WorkspaceManifest>,
) -> Result<Option<Vec<pacquet_workspace::Project>>, pacquet_workspace::FindWorkspaceProjectsError>
{
    let Some(manifest) = workspace_manifest else { return Ok(None) };
    let opts = pacquet_workspace::FindWorkspaceProjectsOpts { patterns: manifest.packages.clone() };
    pacquet_workspace::find_workspace_projects(workspace_root, &opts).map(Some)
}

/// Run every workspace project's own lifecycle scripts after the
/// dependency graph is materialized and bins are linked. Ports the
/// `runLifecycleHooksConcurrently(['preinstall', ...])` call pnpm fires
/// near the end of the install, gated on `!ignoreScripts`:
/// <https://github.com/pnpm/pnpm/blob/80037699fb/pkg-manager/core/src/install/index.ts#L1517-L1530>.
///
/// pacquet has no `ignoreScripts` toggle yet (it hardcodes
/// `ignore_scripts: false` throughout the dependency-build path), so
/// this always runs — matching pnpm's default. Projects are visited
/// root-first; pnpm orders them by `buildIndex` (workspace
/// topological order) and re-links each project's bins between groups
/// so a later project's `prepare` can consume a dependency workspace
/// package's freshly-built output. That ordering — and running
/// projects concurrently under `child_concurrency` — is a follow-up
/// once pacquet computes a per-importer build index; the common
/// single-project case is unaffected.
fn run_projects_lifecycle_scripts<Reporter: self::Reporter>(
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
    config: &Config,
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
    let extra_env = std::collections::HashMap::new();
    for (project_dir, _manifest) in project_manifests {
        let root_modules_dir = project_dir.join(modules_dir_basename);
        let dep_path = project_dir.to_string_lossy();
        run_project_lifecycle_scripts::<Reporter>(&RunPostinstallHooks {
            dep_path: &dep_path,
            pkg_root: project_dir,
            root_modules_dir: &root_modules_dir,
            init_cwd: workspace_root,
            extra_bin_paths: &[],
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
    }
    Ok(())
}

/// Assemble the `(root_dir, manifest)` list every importer the
/// install would walk. Always includes the root manifest; adds each
/// sibling project from `workspace_projects` when present. The root
/// importer always reuses the in-memory `Install.manifest` — `pacquet
/// add` mutates that value before calling install, so re-reading from
/// disk would walk the pre-add shape.
///
/// `workspace_projects.is_none()` covers single-project installs (no
/// `pnpm-workspace.yaml`) — the only manifest is the root one.
fn build_project_manifests_list<'a>(
    workspace_root: &std::path::Path,
    root_manifest: &'a PackageManifest,
    workspace_projects: Option<&'a [pacquet_workspace::Project]>,
) -> Vec<(std::path::PathBuf, &'a PackageManifest)> {
    let mut list = vec![(workspace_root.to_path_buf(), root_manifest)];
    if let Some(projects) = workspace_projects {
        for project in projects {
            if project.root_dir == *workspace_root {
                continue;
            }
            list.push((project.root_dir.clone(), &project.manifest));
        }
    }
    list
}

/// Build the `name → version → WorkspacePackage` lookup the npm
/// resolver consults for `workspace:` specs. Returns `None` when
/// `projects` is `None` (no workspace) so any `workspace:` spec the
/// manifest happens to carry surfaces
/// [`pacquet_resolving_npm_resolver::ResolveFromWorkspaceError::WorkspacePackagesNotLoaded`].
///
/// Mirrors the slice pnpm's
/// [`getWorkspacePackagesByDirectory`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/installing/context/src/index.ts#L160)
/// passes into `resolveDependencies` — same name/version index, same
/// per-project `WorkspacePackage` shape (`{ rootDir, manifest }`).
/// Projects whose manifest lacks a name or version are silently
/// skipped; upstream's manifest reader emits a separate warning that
/// pacquet doesn't carry through here.
fn build_workspace_packages_map(
    projects: Option<&[pacquet_workspace::Project]>,
) -> Option<pacquet_resolving_resolver_base::WorkspacePackages> {
    let projects = projects?;
    let mut map: pacquet_resolving_resolver_base::WorkspacePackages =
        std::collections::BTreeMap::new();
    for project in projects {
        let name = manifest_string_field(&project.manifest, "name");
        let version = manifest_string_field(&project.manifest, "version");
        let (Some(name), Some(version)) = (name, version) else { continue };
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

/// Build the `projects` map for [`WorkspaceState`] from the
/// in-memory `(root_dir, manifest)` list the caller already
/// assembled. Mirrors upstream's
/// `Object.fromEntries(opts.allProjects.map(...))` at
/// <https://github.com/pnpm/pnpm/blob/7ff112bac6/workspace/state/src/createWorkspaceState.ts>.
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
/// settings the install used. Mirrors upstream's `createWorkspaceState`
/// at <https://github.com/pnpm/pnpm/blob/7ff112bac6/workspace/state/src/createWorkspaceState.ts>.
/// Settings pacquet does not track yet (e.g. `peersSuffixMaxLength`)
/// are omitted; pnpm's `checkDepsStatus`
/// only iterates fields present in the serialized object, so an
/// absent key is silently skipped rather than treated as a drift.
pub(crate) fn build_workspace_state(
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
) -> WorkspaceState {
    WorkspaceState {
        last_validated_timestamp: now_millis(),
        projects: build_projects_map(project_manifests),
        // Pacquet doesn't run pnpmfiles yet; record the empty list so
        // pnpm's `patchesOrHooksAreModified` doesn't trip on a missing
        // field.
        pnpmfiles: Vec::new(),
        // Pacquet has no `--filter` yet (issue <https://github.com/pnpm/pacquet/issues/299> stage 2). Hard-code
        // `false` so pnpm doesn't treat the install as partial and
        // skip the cache.
        filtered_install: false,
        config_dependencies: config.config_dependencies.clone(),
        // Settings construction is shared with
        // `optimistic_repeat_install::current_settings` so the
        // freshness check sees the same byte shape this writer
        // produces. Keeping the construction in one place guarantees
        // adding a field on one side doesn't silently flip the other
        // into "drift" on the next install.
        settings: crate::optimistic_repeat_install::current_settings(config, node_linker, included),
    }
}

#[cfg(test)]
mod tests;
