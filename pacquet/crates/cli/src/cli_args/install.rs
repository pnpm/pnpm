use crate::{State, cli_args::supported_architectures::SupportedArchitecturesArgs};
use clap::{Args, ValueEnum};
use derive_more::{Display, Error};
use miette::{Context, Diagnostic};
use pacquet_config::NodeLinker;
use pacquet_lockfile::{Lockfile, LockfileResolution, MaybeLazyLockfile};
use pacquet_package_manager::{
    Install, InstallFrozenLockfileError, LockfileVerificationOverride, TarballPrefetcher,
    UpToDateFastPathCheck, UpdateSeedPolicy, install_already_up_to_date,
};
use pacquet_package_manifest::DependencyGroup;
use pacquet_pnpr_client::{PnprClient, PnprClientError, ResolveOptions, VerifyLockfileOptions};
use pacquet_reporter::Reporter;

const BENCHMARK_PNPR_SERVER_REGISTRY_ENV: &str = "PACQUET_BENCHMARK_PNPR_SERVER_REGISTRY";
const BENCHMARK_PNPR_TARBALL_REWRITE_FROM_ENV: &str = "PACQUET_BENCHMARK_PNPR_TARBALL_REWRITE_FROM";

/// `--node-linker` value parser. CLI mirror of
/// [`pacquet_config::NodeLinker`] so the config crate stays free
/// of `clap` as a dependency. Converted to the canonical enum at
/// the CLI/Install boundary via [`Self::into_config`].
#[derive(Debug, Clone, Copy, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum NodeLinkerArg {
    Isolated,
    Hoisted,
    Pnp,
}

impl NodeLinkerArg {
    #[inline]
    fn into_config(self) -> NodeLinker {
        match self {
            NodeLinkerArg::Isolated => NodeLinker::Isolated,
            NodeLinkerArg::Hoisted => NodeLinker::Hoisted,
            NodeLinkerArg::Pnp => NodeLinker::Pnp,
        }
    }
}

#[derive(Debug, Args)]
pub struct InstallDependencyOptions {
    /// pacquet will not install any package listed in devDependencies and will remove those insofar
    /// they were already installed, if the `NODE_ENV` environment variable is set to production.
    /// Use this flag to instruct pacquet to ignore `NODE_ENV` and take its production status from this
    /// flag instead.
    #[arg(short = 'P', long)]
    prod: bool,
    /// Only devDependencies are installed and dependencies are removed insofar they were
    /// already installed, regardless of the `NODE_ENV`.
    #[arg(short = 'D', long)]
    dev: bool,
    /// optionalDependencies are not installed.
    #[arg(long)]
    no_optional: bool,
}

impl InstallDependencyOptions {
    /// Convert the dependency options to an iterator of [`DependencyGroup`]
    /// which filters the types of dependencies to install.
    fn dependency_groups(&self) -> impl Iterator<Item = DependencyGroup> {
        let &InstallDependencyOptions { prod, dev, no_optional } = self;
        let has_both = prod == dev;
        let has_prod = has_both || prod;
        let has_dev = has_both || dev;
        let has_optional = !no_optional;
        std::iter::empty()
            .chain(has_prod.then_some(DependencyGroup::Prod))
            .chain(has_dev.then_some(DependencyGroup::Dev))
            .chain(has_optional.then_some(DependencyGroup::Optional))
    }
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    #[clap(flatten)]
    pub dependency_options: InstallDependencyOptions,

    /// `--cpu` / `--os` / `--libc` overrides for the optional-dep
    /// platform filter. Mirrors upstream pnpm's CLI flags; merges
    /// per-axis into `supportedArchitectures` loaded from
    /// `pnpm-workspace.yaml`.
    #[clap(flatten)]
    pub supported_architectures: SupportedArchitecturesArgs,

    /// Don't generate a lockfile and fail if the lockfile is outdated.
    #[clap(long)]
    pub frozen_lockfile: bool,

    /// Dependencies are not downloaded. Only `pnpm-lock.yaml` is
    /// updated. Resolution still runs, but nothing is fetched into the
    /// store and no `node_modules` is created. Mirrors pnpm's
    /// `--lockfile-only`.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,

    /// Report what an install would change without writing anything to
    /// disk (no `pnpm-lock.yaml`, no `node_modules`). Resolution still
    /// runs against the registry. Exits 0 whether or not changes were
    /// found. Mirrors pnpm's `install --dry-run`.
    #[clap(long = "dry-run")]
    pub dry_run: bool,

    /// Force-enable `preferFrozenLockfile` for this invocation.
    /// Overrides `pnpm-workspace.yaml` / `PNPM_CONFIG_PREFER_FROZEN_LOCKFILE`.
    /// Mirrors pnpm's `--prefer-frozen-lockfile`. Conflicts with
    /// [`Self::no_prefer_frozen_lockfile`] so a single invocation
    /// can't both force-on and force-off.
    #[clap(long = "prefer-frozen-lockfile")]
    pub prefer_frozen_lockfile: bool,

    /// Force-disable `preferFrozenLockfile` for this invocation.
    /// Overrides `pnpm-workspace.yaml` / `PNPM_CONFIG_PREFER_FROZEN_LOCKFILE`.
    /// Mirrors pnpm's `--no-prefer-frozen-lockfile`. Useful for CI
    /// runs that want to force a re-resolve against the registry
    /// without setting the flag globally.
    #[clap(long = "no-prefer-frozen-lockfile", conflicts_with = "prefer_frozen_lockfile")]
    pub no_prefer_frozen_lockfile: bool,

    /// Skip the per-importer `package.json` ↔ `pnpm-lock.yaml`
    /// freshness check that normally guards `--frozen-lockfile`.
    /// Intended for callers that just resolved and wrote the
    /// lockfile themselves (today: the pnpm CLI delegating
    /// materialization to pacquet via `configDependencies`), where
    /// the manifest may still be the pre-mutation copy while the
    /// lockfile is already post-mutation — the upstream resolver
    /// will rewrite the manifest right after pacquet returns. See
    /// <https://github.com/pnpm/pnpm/issues/11797>.
    ///
    /// Narrow on purpose: only gates
    /// [`pacquet_lockfile::satisfies_package_manifest`]. Settings
    /// drift (`overrides`, `ignoredOptionalDependencies`,
    /// `pnpmfileChecksum`, ...) still aborts. A future broader flag
    /// matching pnpm's internal `ignorePackageManifest` (used by
    /// `pnpm fetch`) would skip linking / hoisting / pruning too;
    /// that's deliberately a separate name.
    #[clap(long)]
    pub ignore_manifest_check: bool,

    /// Skip the install of any runtime dependencies
    /// (`node@runtime:`, `deno@runtime:`, `bun@runtime:`).
    /// Their archives aren't fetched, their slots aren't
    /// materialized, and their bins aren't linked into
    /// `node_modules/.bin/`. The rest of the install proceeds
    /// normally. Mirrors pnpm's `--no-runtime` flag.
    #[clap(long = "no-runtime")]
    pub no_runtime: bool,

    /// Don't run lifecycle scripts of the project or its dependencies.
    /// Dependency build scripts that would otherwise be reported as
    /// ignored are skipped silently, so the install doesn't fail with
    /// `ERR_PNPM_IGNORED_BUILDS` under `strictDepBuilds`. Mirrors pnpm's
    /// `--ignore-scripts`.
    ///
    /// Merged into `config.ignore_scripts` at the CLI dispatch in
    /// `cli_args.rs` (it only ever enables, never toggles a yaml `true`
    /// back off), so the install reads it from the config like every
    /// other build-script setting.
    #[clap(long = "ignore-scripts")]
    pub ignore_scripts: bool,

    /// Override `nodeLinker` from `pnpm-workspace.yaml` /
    /// `.npmrc`. Mirrors upstream pnpm's `--node-linker` flag.
    /// `None` (flag not passed) leaves the config's value
    /// untouched; otherwise the CLI value wins for this invocation
    /// and is what gets written to `.modules.yaml.nodeLinker`.
    /// `isolated` is the default, `hoisted` selects the flat
    /// `node_modules/` layout, `pnp` selects Plug'n'Play.
    #[clap(long = "node-linker", value_enum)]
    pub node_linker: Option<NodeLinkerArg>,

    /// Refuse network tarball / zip-archive fetches on a cache miss.
    /// When the warm prefetch and the `index.db` lookup both miss
    /// for a package, pacquet fails with
    /// `ERR_PACQUET_NO_OFFLINE_TARBALL` rather than hitting the
    /// registry. Mirrors pnpm's
    /// [`--offline`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/npm-resolver/src/pickPackage.ts)
    /// in spirit; upstream's flag gates the metadata-fetch path
    /// (`ERR_PNPM_NO_OFFLINE_META`), which pacquet doesn't have on
    /// the frozen-install flow (the lockfile pins every
    /// resolution), so this flag is currently scoped to artifact
    /// fetches. Stage 2's resolver will extend the gate to the
    /// metadata path, matching upstream byte-for-byte.
    ///
    /// Overrides `offline` from `pnpm-workspace.yaml`: any
    /// `--offline` upgrades a yaml `false` to `true`, but cannot
    /// turn an explicit yaml `true` back off.
    #[clap(long)]
    pub offline: bool,

    /// Open the package store read-only (immutable) and skip all store
    /// writes. For installs against a store on a read-only filesystem
    /// (e.g. a Nix store); pair with `--offline --frozen-lockfile`.
    /// Mirrors pnpm's `--frozen-store`. Overrides `frozenStore` from
    /// `pnpm-workspace.yaml`: any `--frozen-store` upgrades a yaml
    /// `false` to `true`, but cannot turn an explicit yaml `true` back
    /// off. (pnpm additionally rejects `--frozen-store` combined with
    /// `--force`; pacquet has no `force` flow yet, so there is nothing
    /// to conflict with — the guard ports alongside `force`.)
    #[clap(long = "frozen-store")]
    pub frozen_store: bool,

    /// Prefer cached artifacts over network fetches when both have
    /// what's needed. Mirrors pnpm's
    /// [`--prefer-offline`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/npm-resolver/src/pickPackage.ts)
    /// in spirit; upstream's flag biases the metadata resolver to
    /// the cached copy past the freshness window. Pacquet's
    /// frozen-install path already prefers the local store via the
    /// warm prefetch + `index.db` lookups, so the flag is a no-op
    /// for artifact fetches today. Field exists so yaml / CLI parse
    /// cleanly; Stage 2's resolver will honor it on the metadata
    /// path the way upstream does.
    #[clap(long)]
    pub prefer_offline: bool,

    /// Skip the lockfile supply-chain verification pass entirely.
    /// Overrides `pnpm-workspace.yaml#trustLockfile`. Mirrors pnpm's
    /// `--trust-lockfile`. See [`pacquet_config::Config::trust_lockfile`].
    /// Added for [pnpm/pnpm#11860](https://github.com/pnpm/pnpm/issues/11860).
    #[clap(long = "trust-lockfile")]
    pub trust_lockfile: bool,

    /// Refresh the integrity checksums recorded in `pnpm-lock.yaml`
    /// from the registry. Mirrors pnpm's `--update-checksums`. Skips
    /// the frozen-lockfile fast path; conflicts with `--frozen-lockfile`.
    #[clap(long = "update-checksums")]
    pub update_checksums: bool,

    /// Maximum number of workspace projects to process in parallel.
    /// Mirrors pnpm's `--workspace-concurrency`. Overrides the
    /// `workspaceConcurrency` value resolved from `pnpm-workspace.yaml` /
    /// global `config.yaml` / `PNPM_CONFIG_WORKSPACE_CONCURRENCY` for
    /// this invocation. A non-positive value is read as
    /// `parallelism - |value|` (floored at 1), matching upstream's
    /// [`getWorkspaceConcurrency`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.ts#L25-L34).
    /// `None` (flag absent) leaves the config-resolved value in place.
    ///
    /// Applied to [`pacquet_config::Config::workspace_concurrency`] at
    /// the CLI dispatch in [`crate::cli_args::CliArgs::run`]; see that
    /// field for why it has no consumption point on `install` yet.
    #[clap(long = "workspace-concurrency")]
    pub workspace_concurrency: Option<i32>,

    /// Maximum number of concurrent network requests during install.
    /// Mirrors pnpm's `--network-concurrency`; overrides the
    /// `networkConcurrency` value resolved from `pnpm-workspace.yaml` /
    /// global `config.yaml` / `PNPM_CONFIG_NETWORK_CONCURRENCY` for this
    /// invocation. `None` (flag absent) leaves the config-resolved
    /// value in place. Applied to
    /// [`pacquet_config::Config::network_concurrency`] at the CLI
    /// dispatch in [`crate::cli_args::CliArgs::run`].
    #[clap(long = "network-concurrency")]
    pub network_concurrency: Option<usize>,

    /// Per-request network timeout in milliseconds. Mirrors pnpm's
    /// `--fetch-timeout`; overrides `fetchTimeout` for this invocation.
    /// Applied to [`pacquet_config::Config::fetch_timeout`].
    #[clap(long = "fetch-timeout")]
    pub fetch_timeout: Option<u64>,

    /// `User-Agent` header sent on registry requests. Mirrors pnpm's
    /// `--user-agent`; overrides `userAgent` for this invocation.
    /// Applied to [`pacquet_config::Config::user_agent`].
    #[clap(long = "user-agent")]
    pub user_agent: Option<String>,

    /// URL of a `pnpr` server to offload resolution + file fetching to.
    /// Overrides the `pnprServer` setting for this invocation. When set,
    /// the server resolves against the client's registries and
    /// `node_modules` is linked locally from the server-produced
    /// lockfile. Applied to [`pacquet_config::Config::pnpr_server`].
    #[clap(long = "pnpr-server")]
    pub pnpr_server: Option<String>,
}

impl InstallArgs {
    pub(crate) fn for_patch_manifest_change() -> Self {
        Self {
            dependency_options: InstallDependencyOptions {
                prod: false,
                dev: false,
                no_optional: false,
            },
            supported_architectures: SupportedArchitecturesArgs::default(),
            frozen_lockfile: false,
            lockfile_only: false,
            dry_run: false,
            prefer_frozen_lockfile: false,
            no_prefer_frozen_lockfile: true,
            ignore_manifest_check: false,
            no_runtime: false,
            ignore_scripts: false,
            node_linker: None,
            offline: false,
            frozen_store: false,
            prefer_offline: false,
            trust_lockfile: false,
            update_checksums: false,
            workspace_concurrency: None,
            network_concurrency: None,
            fetch_timeout: None,
            user_agent: None,
            pnpr_server: None,
        }
    }

    /// Run the repeat-install fast path before any of the async install
    /// machinery exists: when every gate below holds and
    /// [`install_already_up_to_date`] confirms nothing changed since the
    /// previous install, emit the same "Already up to date" + summary
    /// events [`Install::run`] would and report the install as finished.
    ///
    /// The gates mirror the dispatch in [`crate::cli_args::CliArgs::run`]
    /// (which checks `recursive` / `filter` before reaching here) plus
    /// every input that would make [`Install::run`] skip its own
    /// short-circuit or do extra pre-install work: an explicit
    /// `--frozen-lockfile` / `--lockfile-only`, a configured pnpr
    /// server (that path never runs the optimistic check), config
    /// dependencies, and pnpmfile `updateConfig` hooks (both can
    /// mutate the config the check compares against).
    ///
    /// `false` means "not decided" — the caller proceeds with the full
    /// install path, which re-runs the same check cheaply and
    /// reproduces any error with its established shape.
    pub fn finished_via_up_to_date_fast_path(
        &self,
        dir: &std::path::Path,
        config: &pacquet_config::Config,
        emit: fn(&pacquet_reporter::LogEvent),
    ) -> bool {
        if self.frozen_lockfile || self.lockfile_only || self.pnpr_server.is_some() {
            return false;
        }
        if config.pnpr_server.is_some() {
            return false;
        }
        if config.config_dependencies.as_ref().is_some_and(|deps| !deps.is_empty()) {
            return false;
        }
        let config_root = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        if pacquet_hooks::finder::find_pnpmfile(&config_root).is_some() {
            return false;
        }
        let manifest_path = dir.join("package.json");
        if !manifest_path.is_file() {
            return false;
        }
        let Ok(manifest) = pacquet_package_manifest::PackageManifest::from_path(manifest_path)
        else {
            return false;
        };
        let node_linker = self.node_linker.map_or(config.node_linker, NodeLinkerArg::into_config);
        let Some(workspace_root) = install_already_up_to_date(&UpToDateFastPathCheck {
            config,
            manifest: &manifest,
            dependency_groups: self.dependency_options.dependency_groups().collect(),
            node_linker,
        }) else {
            return false;
        };
        let prefix = workspace_root.to_string_lossy().into_owned();
        emit(&pacquet_reporter::LogEvent::Pnpm(pacquet_reporter::PnpmLog {
            level: pacquet_reporter::LogLevel::Info,
            message: "Already up to date".to_string(),
            prefix: prefix.clone(),
        }));
        emit(&pacquet_reporter::LogEvent::Summary(pacquet_reporter::SummaryLog {
            level: pacquet_reporter::LogLevel::Debug,
            prefix,
        }));
        true
    }

    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;
        let InstallArgs {
            dependency_options,
            supported_architectures,
            frozen_lockfile,
            lockfile_only,
            dry_run,
            prefer_frozen_lockfile,
            no_prefer_frozen_lockfile,
            ignore_manifest_check,
            no_runtime,
            // Read from `config.ignore_scripts` (the CLI flag was already
            // merged in by the dispatch in `cli_args.rs`), not from here.
            ignore_scripts: _,
            node_linker,
            offline: _,
            // Read from `config.frozen_store` (the CLI flag was already
            // merged in by the dispatch in `cli_args.rs`), not from here.
            frozen_store: _,
            prefer_offline: _,
            trust_lockfile,
            update_checksums,
            workspace_concurrency: _,
            network_concurrency: _,
            fetch_timeout: _,
            user_agent: _,
            // Read from `config.pnpr_server` (the CLI flag was already
            // merged in by the dispatch in `cli_args.rs`), not from here.
            pnpr_server: _,
        } = self;

        // `--prefer-frozen-lockfile` / `--no-prefer-frozen-lockfile`
        // map to `Option<bool>`: `Some(true)` / `Some(false)` when
        // either flag is set, `None` otherwise (use config). Clap's
        // `conflicts_with` on the off-flag ensures the two aren't
        // both set, so the precedence here is straightforward.
        let prefer_frozen_lockfile = if prefer_frozen_lockfile {
            Some(true)
        } else if no_prefer_frozen_lockfile {
            Some(false)
        } else {
            None
        };

        // Merge CLI overrides with the yaml-derived value before
        // handing off to the install pipeline. `state.config` is a
        // shared `&'static Config`, so we compute the effective
        // `SupportedArchitectures` from a clone instead of mutating
        // in place; the install path takes the merged value as an
        // explicit parameter.
        let supported_architectures =
            supported_architectures.apply_to(config.supported_architectures.clone());

        // Either the npmrc/yaml-derived setting or the CLI flag
        // turns runtime-skipping on; pacquet doesn't expose a way
        // to override yaml's `true` back to `false` from the CLI,
        // matching pnpm's stance on the same flag.
        let skip_runtimes = config.skip_runtimes || no_runtime;

        // Same shape as `skip_runtimes`: yaml `trustLockfile: true`
        // or the CLI flag turns the verification skip on. There's no
        // CLI inverse — relax the yaml value if you need to flip it
        // back off for a single invocation.
        let trust_lockfile = config.trust_lockfile || trust_lockfile;

        // `--node-linker` flag (if passed) overrides the
        // yaml/npmrc value for this invocation. Mirrors pnpm's
        // override-on-explicit-flag semantics.
        let node_linker = node_linker.map_or(config.node_linker, NodeLinkerArg::into_config);
        // The lockfile-verification gate keys its on-disk cache off
        // `<manifest_dir>/pnpm-lock.yaml`. Once workspace support
        // lands (pacquet#431), this becomes `workspace_root` to
        // match where the lockfile actually lives.
        let lockfile_path = manifest
            .path()
            .parent()
            .map(|parent| parent.join(pacquet_lockfile::Lockfile::FILE_NAME));

        // pnpr fast path: when a `pnprServer` URL is configured, offload
        // resolution + fetching to it, then link `node_modules` from the
        // server-produced lockfile via the normal frozen install. Mirrors
        // pnpm's `install()` delegating to `installFromPnpmRegistry`.
        if let Some(pnpr_server) = config.pnpr_server.as_deref() {
            // The pnpr path resolves and links through the server, so it
            // can't honor `--dry-run`'s no-write contract. Reject up front,
            // mirroring pnpm's CONFIG_CONFLICT_DRY_RUN_WITH_PNPR_SERVER.
            if dry_run {
                return Err(DryRunIncompatibleWithPnpr.into());
            }
            return install_via_pnpr::<Reporter>(
                &state,
                pnpr_server,
                PnprLink {
                    dependency_groups: dependency_options.dependency_groups().collect(),
                    supported_architectures,
                    node_linker,
                    skip_runtimes,
                    frozen_lockfile,
                    prefer_frozen_lockfile: prefer_frozen_lockfile
                        .unwrap_or(config.prefer_frozen_lockfile),
                    lockfile_only,
                    ignore_manifest_check,
                    trust_lockfile,
                    lockfile_path: lockfile_path.as_deref(),
                },
            )
            .await;
        }

        Install {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            lockfile: MaybeLazyLockfile::Lazy(lockfile),
            lockfile_path: lockfile_path.as_deref(),
            dependency_groups: dependency_options.dependency_groups(),
            frozen_lockfile,
            prefer_frozen_lockfile,
            ignore_manifest_check,
            skip_runtimes,
            trust_lockfile,
            update_checksums,
            // `pacquet install` is always a full install (it takes no
            // package arguments), so the project's own lifecycle
            // scripts run. `pacquet add` sets this to `false`.
            is_full_install: true,
            resolved_packages,
            supported_architectures,
            node_linker,
            lockfile_only,
            dry_run,
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            auth_override: None,
            resolution_observer: None,
            catalogs_override: None,
        }
        .run::<Reporter>()
        .await
        .wrap_err("installing dependencies")?;

        Ok(())
    }

    /// Effective `workspaceConcurrency` for this invocation: the
    /// `--workspace-concurrency` flag when passed (resolved through
    /// [`pacquet_config::resolve_child_concurrency`], so a non-positive
    /// value means `parallelism - |value|`, floored at 1), otherwise
    /// the already-resolved `config_value` from `pnpm-workspace.yaml` /
    /// global `config.yaml` / `PNPM_CONFIG_WORKSPACE_CONCURRENCY`.
    ///
    /// Mirrors upstream's final `workspaceConcurrency =
    /// getWorkspaceConcurrency(...)` pass at
    /// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L641>.
    pub(crate) fn resolve_workspace_concurrency(&self, config_value: u32) -> u32 {
        match self.workspace_concurrency {
            Some(value) => pacquet_config::resolve_child_concurrency(Some(value)),
            None => config_value,
        }
    }
}

/// Per-invocation install knobs forwarded to the frozen link pass,
/// already resolved from the CLI flags + config by [`InstallArgs::run`].
struct PnprLink<'a> {
    dependency_groups: Vec<DependencyGroup>,
    supported_architectures: Option<pacquet_package_is_installable::SupportedArchitectures>,
    node_linker: NodeLinker,
    skip_runtimes: bool,
    /// Governs the *server's* resolution behavior (frozen vs
    /// reuse-and-update); forwarded to `/-/pnpr/v0/resolve`. The local
    /// materialization always runs frozen against the server-produced
    /// lockfile.
    frozen_lockfile: bool,
    /// The *effective* `preferFrozenLockfile` (the CLI tri-state already
    /// resolved against `config.prefer_frozen_lockfile`, exactly as the
    /// local `Install` resolves it); forwarded to `/-/pnpr/v0/resolve`. `false`
    /// forces the server to re-resolve. Resolving here — rather than
    /// sending the raw CLI override — keeps a yaml `preferFrozenLockfile:
    /// false` honored on the pnpr path without `--no-prefer-frozen-lockfile`.
    prefer_frozen_lockfile: bool,
    /// `--lockfile-only`. Forwarded to `/-/pnpr/v0/resolve` so the server
    /// resolves only — returning the lockfile without fetching files —
    /// after which `install_via_pnpr` writes the lockfile and skips
    /// materialization, mirroring pnpm's resolve + write, fetch nothing,
    /// link nothing. See
    /// [pnpm/pnpm#12146](https://github.com/pnpm/pnpm/issues/12146).
    lockfile_only: bool,
    /// `--ignore-manifest-check`; forwarded so the server's frozen
    /// freshness check and the local materialization both skip the
    /// manifest ↔ lockfile comparison.
    ignore_manifest_check: bool,
    /// The effective `trustLockfile` (yaml `trustLockfile` OR
    /// `--trust-lockfile`); forwarded so the server skips verifying the
    /// input lockfile when the user opted out, mirroring the local path.
    trust_lockfile: bool,
    lockfile_path: Option<&'a std::path::Path>,
}

/// `frozenStore` was enabled together with a configured `pnprServer`.
/// The pnpr path writes resolved files into the store, which `frozenStore`
/// opens read-only, so the combination can't proceed. Mirrors pnpm's
/// `ERR_PNPM_FROZEN_STORE_INCOMPATIBLE_WITH_PNPR`.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "The pnpr server resolves dependencies and writes new entries into the store, which is opened read-only when frozenStore is enabled."
)]
#[diagnostic(
    code(ERR_PNPM_FROZEN_STORE_INCOMPATIBLE_WITH_PNPR),
    help(
        "Disable the pnpr server (unset `--pnpr-server` / `pnprServer` in pnpm-workspace.yaml) so the install reads from the existing store, or unset `frozenStore` to allow store writes."
    )
)]
struct FrozenStoreIncompatibleWithPnpr;

/// `--dry-run` was requested with a configured `pnprServer`. The pnpr path
/// resolves and links through the server, so it can't honor the dry-run
/// "writes nothing" contract. Mirrors pnpm's
/// `ERR_PNPM_CONFIG_CONFLICT_DRY_RUN_WITH_PNPR_SERVER`.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Cannot use --dry-run with a configured pnpr server because the pnpr install path resolves and links through the server."
)]
#[diagnostic(
    code(ERR_PNPM_CONFIG_CONFLICT_DRY_RUN_WITH_PNPR_SERVER),
    help(
        "Unset the pnpr server (`--pnpr-server` / `pnprServer` in pnpm-workspace.yaml) to preview locally, or drop --dry-run."
    )
)]
struct DryRunIncompatibleWithPnpr;

/// Resolve a single project through a `pnpr` server, then link it.
///
/// Sends the client's registries to the server, which resolves against
/// them and returns the resolved lockfile; writes that lockfile, then
/// runs a frozen install to materialize `node_modules` from it — the
/// frozen install fetches every tarball from the registries itself, like
/// a normal install. This is the equivalent of pnpm's
/// `installFromPnpmRegistry` handing off to `headlessInstall`. Under
/// `--lockfile-only` it stops after writing the lockfile (fetch nothing,
/// link nothing).
async fn install_via_pnpr<Reporter: self::Reporter + 'static>(
    state: &State,
    pnpr_server: &str,
    link: PnprLink<'_>,
) -> miette::Result<()> {
    // The pnpr server resolves dependencies and streams missing files
    // straight into the store, so this path inherently writes the store.
    // `frozenStore` promises the store is complete and read-only, so the
    // two are mutually exclusive — refuse up front instead of failing on
    // the read-only write. Mirrors pnpm's `FROZEN_STORE_INCOMPATIBLE_WITH_PNPR`
    // guard in `installFromPnpmRegistry`.
    if state.config.frozen_store {
        return Err(FrozenStoreIncompatibleWithPnpr.into());
    }

    let dependencies = state
        .manifest
        .dependencies([DependencyGroup::Prod])
        .map(|(name, spec)| (name.to_string(), spec.to_string()))
        .collect();
    let dev_dependencies = state
        .manifest
        .dependencies([DependencyGroup::Dev])
        .map(|(name, spec)| (name.to_string(), spec.to_string()))
        .collect();
    let optional_dependencies = state
        .manifest
        .dependencies([DependencyGroup::Optional])
        .map(|(name, spec)| (name.to_string(), spec.to_string()))
        .collect();

    let overrides = state
        .config
        .overrides
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|err| miette::miette!("failed to serialize overrides: {err}"))?;
    let benchmark_registry_override =
        PnprBenchmarkRegistryOverride::from_env(&state.config.registry);
    let resolve_registry = benchmark_registry_override.as_ref().map_or_else(
        || state.config.registry.clone(),
        PnprBenchmarkRegistryOverride::resolve_registry,
    );

    // Send the on-disk lockfile + the full client policy so the server
    // verifies the input lockfile under *our* policy before resolving;
    // the client never runs `verify_lockfile_resolutions` on the pnpr
    // path ([pnpm/pnpm#12139](https://github.com/pnpm/pnpm/issues/12139)).
    // `trustPolicy: no-downgrade` is enforced
    // server-side now — both for reused entries (the input-lockfile
    // verifier) and freshly-resolved ones (the resolver's pick-time
    // gate, since the policy is wired into the server's config).
    let opts = ResolveOptions {
        dependencies,
        dev_dependencies,
        optional_dependencies,
        registry: resolve_registry,
        named_registries: state.config.named_registries.clone(),
        // Forward the whole credential map: the registries a graph
        // touches aren't known up front (scope-routed or tarball-URL
        // sub-deps), so the server attaches the right token per URL.
        auth_headers: state.config.auth_headers.to_by_scope(),
        authorization: state.config.auth_headers.for_url(pnpr_server),
        overrides,
        lockfile: state
            .lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?
            .cloned(),
        frozen_lockfile: link.frozen_lockfile,
        prefer_frozen_lockfile: Some(link.prefer_frozen_lockfile),
        ignore_manifest_check: link.ignore_manifest_check,
        trust_lockfile: link.trust_lockfile,
        minimum_release_age: state.config.minimum_release_age,
        minimum_release_age_exclude: state.config.minimum_release_age_exclude.clone(),
        minimum_release_age_ignore_missing_time: state
            .config
            .minimum_release_age_ignore_missing_time,
        trust_policy: state.config.trust_policy,
        trust_policy_exclude: state.config.trust_policy_exclude.clone(),
        trust_policy_ignore_after: state.config.trust_policy_ignore_after,
    };

    let client = PnprClient::new(pnpr_server);
    let lockfile_dir =
        state.manifest.path().parent().expect("manifest path always has a parent dir");

    if link.frozen_lockfile
        && !link.lockfile_only
        && let Some(lockfile) = state
            .lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?
    {
        let prefetcher = TarballPrefetcher::new(
            state.config,
            &state.http_client,
            &state.tarball_mem_cache,
            None,
            &lockfile_dir.to_string_lossy(),
        )
        .await;
        prefetcher.prefetch_lockfile(lockfile, state.config).await;
        tokio::task::yield_now().await;

        let lockfile_verification_override: Option<LockfileVerificationOverride<'_>> =
            if link.trust_lockfile {
                None
            } else {
                let verify_opts = VerifyLockfileOptions::from_resolve_options(&opts)
                    .expect("frozen pnpr verification requires the loaded lockfile");
                let verify_client = PnprClient::new(pnpr_server);
                Some(Box::pin(async move {
                    match verify_client.verify_lockfile(verify_opts).await {
                        Ok(()) => Ok(()),
                        Err(PnprClientError::Verification(verify_err)) => {
                            Err(InstallFrozenLockfileError::LockfileVerification(verify_err))
                        }
                        Err(err) => Err(InstallFrozenLockfileError::ExternalLockfileVerification(
                            err.to_string(),
                        )),
                    }
                }) as LockfileVerificationOverride<'_>)
            };

        let install = Install {
            tarball_mem_cache: std::sync::Arc::clone(&state.tarball_mem_cache),
            http_client: &state.http_client,
            http_client_arc: std::sync::Arc::clone(&state.http_client),
            config: state.config,
            manifest: &state.manifest,
            lockfile: MaybeLazyLockfile::Loaded(Some(lockfile)),
            lockfile_path: link.lockfile_path,
            dependency_groups: link.dependency_groups,
            frozen_lockfile: true,
            prefer_frozen_lockfile: None,
            ignore_manifest_check: link.ignore_manifest_check,
            skip_runtimes: link.skip_runtimes,
            trust_lockfile: true,
            update_checksums: false,
            is_full_install: true,
            resolved_packages: &state.resolved_packages,
            supported_architectures: link.supported_architectures,
            node_linker: link.node_linker,
            lockfile_only: false,
            dry_run: false,
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            auth_override: None,
            resolution_observer: None,
            catalogs_override: None,
        };

        let result = match lockfile_verification_override {
            Some(lockfile_verification_override) => {
                install
                    .run_with_lockfile_verification::<Reporter>(lockfile_verification_override)
                    .await
            }
            None => install.run::<Reporter>().await,
        };
        // On failure the prefetcher is dropped, not shut down: shutdown
        // waits for every in-flight prefetch download (each task holds a
        // store-index writer handle), which would hold the fail-fast
        // abort hostage to the remaining transfers. The index rows are
        // best-effort — a dropped row only costs a later re-download.
        result.wrap_err("restoring dependencies from the local lockfile via pnpr verification")?;

        prefetcher.shutdown().await;

        return Ok(());
    }

    // Under `--lockfile-only` nothing is materialized, so skip the
    // prefetcher entirely and consume the stream with a no-op callback.
    // Otherwise spawn a prefetcher that fires each tarball download as
    // its `package` frame streams in, so fetch overlaps the server's
    // resolution ([pnpm/pnpm#12234](https://github.com/pnpm/pnpm/issues/12234));
    // the frozen materialization install below then finds every tarball
    // already in the shared mem cache.
    let prefetcher = if link.lockfile_only {
        None
    } else {
        Some(
            TarballPrefetcher::new(
                state.config,
                &state.http_client,
                &state.tarball_mem_cache,
                None,
                &lockfile_dir.to_string_lossy(),
            )
            .await,
        )
    };

    let result = match prefetcher.as_ref() {
        Some(prefetcher) => {
            client
                .resolve_streaming(opts, |pkg| {
                    let tarball = benchmark_registry_override.as_ref().map_or_else(
                        || pkg.tarball.clone(),
                        |registry| registry.client_tarball_url(&pkg.tarball),
                    );
                    prefetcher.prefetch(
                        pkg.id,
                        tarball,
                        &pkg.integrity,
                        pkg.unpacked_size,
                        pkg.file_count,
                    );
                })
                .await
        }
        None => client.resolve(opts).await,
    };
    let mut outcome = match result {
        Ok(outcome) => outcome,
        // The server rejected the input lockfile under our policy.
        // Surface the reconstructed `VerifyError` so the abort + the
        // `ERR_PNPM_*` diagnostic code match the local gate exactly.
        Err(PnprClientError::Verification(verify_err)) => {
            return Err(miette::Report::new(verify_err));
        }
        Err(err) => {
            return Err(miette::miette!("{err}"))
                .wrap_err("resolving dependencies via the pnpr server");
        }
    };
    if let Some(registry) = benchmark_registry_override.as_ref() {
        registry.rewrite_lockfile(&mut outcome.lockfile);
    }

    if state.config.lockfile {
        outcome
            .lockfile
            .save_to_path(&lockfile_dir.join(Lockfile::FILE_NAME))
            .map_err(|err| miette::miette!("{err}"))
            .wrap_err("writing the pnpr-resolved lockfile")?;
    }

    // `--lockfile-only`: the server resolved and returned the lockfile
    // but fetched nothing; pnpm links nothing in this mode, so stop after
    // writing the lockfile rather than running the materialization pass.
    // See [pnpm/pnpm#12146](https://github.com/pnpm/pnpm/issues/12146).
    if link.lockfile_only {
        return Ok(());
    }

    Install {
        tarball_mem_cache: std::sync::Arc::clone(&state.tarball_mem_cache),
        http_client: &state.http_client,
        http_client_arc: std::sync::Arc::clone(&state.http_client),
        config: state.config,
        manifest: &state.manifest,
        lockfile: MaybeLazyLockfile::Loaded(Some(&outcome.lockfile)),
        lockfile_path: link.lockfile_path,
        dependency_groups: link.dependency_groups,
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: link.ignore_manifest_check,
        skip_runtimes: link.skip_runtimes,
        // The server already verified the input lockfile and resolved
        // the rest under our policy, so the local materialization treats
        // the server-produced lockfile as trusted — it never re-runs
        // `verify_lockfile_resolutions` or touches the local
        // `lockfile-verified.jsonl` cache
        // ([pnpm/pnpm#12139](https://github.com/pnpm/pnpm/issues/12139)).
        trust_lockfile: true,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &state.resolved_packages,
        supported_architectures: link.supported_architectures,
        node_linker: link.node_linker,
        lockfile_only: false,
        dry_run: false,
        update_seed_policy: UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
        catalogs_override: None,
    }
    .run::<Reporter>()
    .await
    .wrap_err("linking dependencies resolved via the pnpr server")?;

    // The materialization install has awaited every tarball's mem-cache
    // slot, so all prefetch downloads have finished and queued their
    // store-index rows. Drain the writer so those rows are persisted for
    // the next install before returning.
    if let Some(prefetcher) = prefetcher {
        prefetcher.shutdown().await;
    }

    Ok(())
}

struct PnprBenchmarkRegistryOverride {
    resolve_registry: String,
    tarball_rewrite: Option<BenchmarkRegistryRewrite>,
}

impl PnprBenchmarkRegistryOverride {
    /// Benchmark-only hook for `pacquet/tasks/integrated-benchmark`.
    ///
    /// The benchmark runs release-built pacquet and pnpr binaries, so this
    /// cannot be hidden behind `#[cfg(test)]`. Keep every
    /// `PACQUET_BENCHMARK_*` env read in this type: normal pnpr installs
    /// take one no-op branch, while benchmark runs can ask the pnpr server
    /// to resolve against a server-side registry URL and then rewrite
    /// server-origin tarball URLs back to the client-facing registry. The
    /// rewrite is applied before saving the lockfile because the benchmark's
    /// frozen materialization must use the same client-registry path that
    /// direct installs pay for.
    fn from_env(client_registry: &str) -> Option<Self> {
        let resolve_registry = std::env::var(BENCHMARK_PNPR_SERVER_REGISTRY_ENV)
            .ok()
            .filter(|registry| !registry.is_empty())
            .map(|registry| normalize_registry(&registry))?;
        let tarball_rewrite_from = std::env::var(BENCHMARK_PNPR_TARBALL_REWRITE_FROM_ENV)
            .ok()
            .filter(|registry| !registry.is_empty());
        let tarball_rewrite = BenchmarkRegistryRewrite::new(
            [Some(resolve_registry.as_str()), tarball_rewrite_from.as_deref()]
                .into_iter()
                .flatten(),
            client_registry,
        );
        Some(Self { resolve_registry, tarball_rewrite })
    }

    fn resolve_registry(&self) -> String {
        self.resolve_registry.clone()
    }

    fn client_tarball_url(&self, url: &str) -> String {
        self.tarball_rewrite.as_ref().map_or_else(|| url.to_string(), |rewrite| rewrite.url(url))
    }

    fn rewrite_lockfile(&self, lockfile: &mut Lockfile) {
        let Some(rewrite) = self.tarball_rewrite.as_ref() else { return };
        let Some(packages) = lockfile.packages.as_mut() else { return };
        for metadata in packages.values_mut() {
            rewrite_resolution_registry(&mut metadata.resolution, rewrite);
        }
    }
}

struct BenchmarkRegistryRewrite {
    from: Vec<String>,
    to: String,
}

impl BenchmarkRegistryRewrite {
    pub(super) fn new<Registry, Registries>(from: Registries, to: &str) -> Option<Self>
    where
        Registry: AsRef<str>,
        Registries: IntoIterator<Item = Registry>,
    {
        let to = normalize_registry(to);
        let mut from_registries = Vec::new();
        for registry in from {
            let registry = normalize_registry(registry.as_ref());
            if registry != to && !from_registries.contains(&registry) {
                from_registries.push(registry);
            }
        }
        (!from_registries.is_empty()).then_some(Self { from: from_registries, to })
    }

    pub(super) fn url(&self, url: &str) -> String {
        self.from
            .iter()
            .find_map(|from| url.strip_prefix(from))
            .map_or_else(|| url.to_string(), |suffix| format!("{}{}", self.to, suffix))
    }
}

fn normalize_registry(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}

fn rewrite_resolution_registry(
    resolution: &mut LockfileResolution,
    rewrite: &BenchmarkRegistryRewrite,
) {
    match resolution {
        LockfileResolution::Tarball(resolution) => {
            resolution.tarball = rewrite.url(&resolution.tarball);
        }
        LockfileResolution::Binary(resolution) => {
            resolution.url = rewrite.url(&resolution.url);
        }
        LockfileResolution::Variations(resolution) => {
            for variant in &mut resolution.variants {
                rewrite_resolution_registry(&mut variant.resolution, rewrite);
            }
        }
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Registry(_) => {}
    }
}

#[cfg(test)]
mod tests;
