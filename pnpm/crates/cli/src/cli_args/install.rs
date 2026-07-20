use crate::{
    State,
    cli_args::{
        pipelines::InstallFamilySelection, recursive::discover_workspace_projects,
        supported_architectures::SupportedArchitecturesArgs,
    },
};
use clap::{Args, ValueEnum};
use derive_more::{Display, Error};
use miette::{Context, Diagnostic};
use pacquet_config::NodeLinker;
use pacquet_lockfile::{Lockfile, LockfileResolution, MaybeLazyLockfile};
use pacquet_modules_yaml::IncludedDependencies;
use pacquet_package_manager::{
    Install, InstallFrozenLockfileError, LockfileVerificationOverride, SkippedSnapshots,
    TarballPrefetcher, UpToDateFastPathCheck, UpdateSeedPolicy, WorkspaceInstallSelection,
    install_already_up_to_date, materialization_closure, merge_filtered_wanted_lockfile,
};
use pacquet_package_manifest::DependencyGroup;
use pacquet_pnpr_client::{
    PnprClient, PnprClientError, ResolveProject, ResolveProjectsOptions, VerifyLockfileOptions,
};
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
    pub(crate) fn into_config(self) -> NodeLinker {
        match self {
            NodeLinkerArg::Isolated => NodeLinker::Isolated,
            NodeLinkerArg::Hoisted => NodeLinker::Hoisted,
            NodeLinkerArg::Pnp => NodeLinker::Pnp,
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct InstallDependencyOptions {
    /// Install only production dependencies. devDependencies are skipped,
    /// and removed if already installed. Takes precedence over `NODE_ENV`.
    #[arg(short = 'P', long)]
    prod: bool,
    /// Install only devDependencies. Regular dependencies are skipped, and
    /// removed if already installed, regardless of `NODE_ENV`.
    #[arg(short = 'D', long)]
    dev: bool,
    /// Don't install optionalDependencies.
    #[arg(long)]
    no_optional: bool,
}

impl InstallDependencyOptions {
    /// Convert the dependency options to an iterator of [`DependencyGroup`]
    /// which filters the types of dependencies to install.
    pub(crate) fn dependency_groups(&self) -> impl Iterator<Item = DependencyGroup> {
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

#[derive(Debug, Clone, Args)]
pub struct InstallArgs {
    #[clap(flatten)]
    pub dependency_options: InstallDependencyOptions,

    /// Restrict which optional dependencies are installed, by CPU
    /// (`--cpu`), OS (`--os`), and C library (`--libc`).
    #[clap(flatten)]
    pub supported_architectures: SupportedArchitecturesArgs,

    /// Don't generate a lockfile, and fail if an update to it is needed.
    #[clap(long)]
    pub frozen_lockfile: bool,

    /// Only update `pnpm-lock.yaml`. Don't download packages or write
    /// `node_modules`.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,

    /// Show what an install would change without writing anything to disk.
    #[clap(long = "dry-run")]
    pub dry_run: bool,

    /// Reinstall every package the lockfile names: relink packages an
    /// earlier install already materialized, and install optional
    /// dependencies whose `cpu` / `os` / `libc` / `engines` don't match
    /// the host instead of skipping them.
    #[clap(long)]
    pub force: bool,

    /// Prefer the existing lockfile over re-resolving, even when the
    /// manifest may have changed.
    #[clap(long = "prefer-frozen-lockfile", overrides_with = "no_prefer_frozen_lockfile")]
    pub prefer_frozen_lockfile: bool,

    /// Always re-resolve against the registry instead of preferring the
    /// existing lockfile.
    #[clap(long = "no-prefer-frozen-lockfile", overrides_with = "prefer_frozen_lockfile")]
    pub no_prefer_frozen_lockfile: bool,

    /// Skip the check that `pnpm-lock.yaml` is up to date with
    /// `package.json` under `--frozen-lockfile`. For callers that just
    /// wrote the lockfile themselves and know the manifest is about to
    /// catch up.
    #[clap(long)]
    pub ignore_manifest_check: bool,

    /// Don't install runtime dependencies (`node`, `deno`, `bun`). Their
    /// archives aren't fetched and their bins aren't linked; the rest of
    /// the install proceeds normally.
    #[clap(long = "no-runtime")]
    pub no_runtime: bool,

    /// Don't run lifecycle scripts of the project or its dependencies.
    /// Packages are still installed; only their build scripts are skipped,
    /// and the install won't fail because of it.
    #[clap(long = "ignore-scripts", overrides_with = "no_ignore_scripts")]
    pub ignore_scripts: bool,

    /// Run lifecycle scripts even when the configuration disables them.
    #[clap(long = "no-ignore-scripts", overrides_with = "ignore_scripts")]
    pub no_ignore_scripts: bool,

    /// Which node linker to use: `isolated` (the default, a symlinked
    /// store), `hoisted` (a flat `node_modules`), or `pnp` (Plug'n'Play).
    /// Overrides the configured value.
    #[clap(long = "node-linker", value_enum)]
    pub node_linker: Option<NodeLinkerArg>,

    /// Fail on a cache miss instead of fetching from the registry, using
    /// only packages already in the store.
    #[clap(long, overrides_with = "no_offline")]
    pub offline: bool,

    /// Allow network fetches even when the configuration enables offline
    /// mode.
    #[clap(long = "no-offline", overrides_with = "offline")]
    pub no_offline: bool,

    /// Open the store read-only and skip all store writes. For installing
    /// against a store on a read-only filesystem (e.g. a Nix store); pair
    /// with `--offline --frozen-lockfile`.
    #[clap(long = "frozen-store", overrides_with = "no_frozen_store")]
    pub frozen_store: bool,

    /// Allow store writes even when the configuration enables the
    /// read-only store.
    #[clap(long = "no-frozen-store", overrides_with = "frozen_store")]
    pub no_frozen_store: bool,

    /// Prefer packages already in the cache over the network, even past
    /// their freshness window.
    #[clap(long, overrides_with = "no_prefer_offline")]
    pub prefer_offline: bool,

    /// Don't prefer cached packages even when the configuration enables
    /// it.
    #[clap(long = "no-prefer-offline", overrides_with = "prefer_offline")]
    pub no_prefer_offline: bool,

    /// Skip verifying the lockfile against supply-chain policies.
    #[clap(long = "trust-lockfile", overrides_with = "no_trust_lockfile")]
    pub trust_lockfile: bool,

    /// Verify the lockfile against supply-chain policies even when the
    /// configuration trusts it.
    #[clap(long = "no-trust-lockfile", overrides_with = "trust_lockfile")]
    pub no_trust_lockfile: bool,

    /// Refresh the integrity checksums in `pnpm-lock.yaml` from the
    /// registry. Cannot be combined with `--frozen-lockfile`.
    #[clap(long = "update-checksums")]
    pub update_checksums: bool,

    /// Maximum number of concurrent network requests during install.
    #[clap(long = "network-concurrency")]
    pub network_concurrency: Option<usize>,

    /// Per-request network timeout, in milliseconds.
    #[clap(long = "fetch-timeout")]
    pub fetch_timeout: Option<u64>,

    /// `User-Agent` header to send on registry requests.
    #[clap(long = "user-agent")]
    pub user_agent: Option<String>,

    /// URL of a pnpr server to offload resolution and file fetching to.
    /// `node_modules` is still linked locally from the server-produced
    /// lockfile.
    #[clap(long = "pnpr-server")]
    pub pnpr_server: Option<String>,
}

/// Resolve a boolean whose CLI surface is a `--flag` / `--no-flag` pair
/// against the yaml/`.npmrc` `config` value. The pair's mutual
/// `overrides_with` collapses both spellings in one argv to the
/// last-specified, so at most one of `force_on` / `force_off` is ever
/// set: a set flag wins over `config` in its own direction and an unset
/// pair falls through to it. Mirrors pnpm, where a CLI boolean overrides
/// the workspace/`.npmrc` value either way (nopt's `--no-` negation).
pub(crate) fn resolve_bool_override(force_on: bool, force_off: bool, config: bool) -> bool {
    force_on || (config && !force_off)
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
            force: false,
            prefer_frozen_lockfile: false,
            no_prefer_frozen_lockfile: true,
            ignore_manifest_check: false,
            no_runtime: false,
            ignore_scripts: false,
            no_ignore_scripts: false,
            node_linker: None,
            offline: false,
            no_offline: false,
            frozen_store: false,
            no_frozen_store: false,
            prefer_offline: false,
            no_prefer_offline: false,
            trust_lockfile: false,
            no_trust_lockfile: false,
            update_checksums: false,
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
        if self.frozen_lockfile || self.lockfile_only || self.force || self.pnpr_server.is_some() {
            return false;
        }
        if config.pnpr_server.is_some() {
            return false;
        }
        // Dedicated per-project lockfiles run one install per workspace
        // project; a single-dir up-to-date probe can't speak for the
        // sibling projects, so the loop (whose per-project engine runs
        // each have their own optimistic short-circuit) must always run.
        if !config.shared_workspace_lockfile && config.workspace_dir.is_some() {
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
            supported_architectures: self
                .supported_architectures
                .apply_to(config.supported_architectures.clone()),
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
        Box::pin(self.run_inner::<Reporter>(state, None)).await
    }

    pub(crate) async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        selection: InstallFamilySelection,
    ) -> miette::Result<()> {
        Box::pin(self.run_inner::<Reporter>(state, Some(selection))).await
    }

    async fn run_inner<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        selection: Option<InstallFamilySelection>,
    ) -> miette::Result<()> {
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;
        let InstallArgs {
            dependency_options,
            supported_architectures,
            frozen_lockfile,
            lockfile_only,
            dry_run,
            // Resolved against config by `apply_install_cli_config` in
            // the dispatch, like `ignore_scripts` below.
            force: _,
            prefer_frozen_lockfile,
            no_prefer_frozen_lockfile,
            ignore_manifest_check,
            no_runtime,
            // The `ignore_scripts` / `offline` / `frozen_store` /
            // `prefer_offline` flags and their `--no-` inverses are
            // resolved against config by `apply_install_cli_config` in the
            // dispatch (`cli_args.rs`), so the install reads them from
            // `config`, not from here.
            ignore_scripts: _,
            no_ignore_scripts: _,
            node_linker,
            offline: _,
            no_offline: _,
            frozen_store: _,
            no_frozen_store: _,
            prefer_offline: _,
            no_prefer_offline: _,
            trust_lockfile,
            no_trust_lockfile,
            update_checksums,
            network_concurrency: _,
            fetch_timeout: _,
            user_agent: _,
            // Read from `config.pnpr_server` (the CLI flag was already
            // merged in by the dispatch in `cli_args.rs`), not from here.
            pnpr_server: _,
        } = self;

        // `--prefer-frozen-lockfile` / `--no-prefer-frozen-lockfile`
        // map to `Option<bool>`: `Some(true)` / `Some(false)` when
        // either flag is set, `None` otherwise (use config). The pair's
        // mutual `overrides_with` collapses both spellings to the
        // last-specified, so at most one is set and the precedence here
        // is straightforward.
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

        // `--trust-lockfile` / `--no-trust-lockfile` override the yaml
        // `trustLockfile` in either direction; an unset pair falls
        // through to it. Forcing the verification pass back on from the
        // CLI matters for security: a repo-controlled
        // `pnpm-workspace.yaml` can't pin `trustLockfile: true` past a
        // user's explicit `--no-trust-lockfile`.
        let trust_lockfile =
            resolve_bool_override(trust_lockfile, no_trust_lockfile, config.trust_lockfile);

        // `--node-linker` flag (if passed) overrides the
        // yaml/npmrc value for this invocation. Mirrors pnpm's
        // override-on-explicit-flag semantics.
        let node_linker = node_linker.map_or(config.node_linker, NodeLinkerArg::into_config);
        let lockfile_path = state.lockfile_path();

        // pnpr fast path: when a `pnprServer` URL is configured, offload
        // resolution + fetching to it, then link `node_modules` from the
        // server-produced lockfile via the normal frozen install.
        if let Some(pnpr_server) = config.pnpr_server.as_deref() {
            // The pnpr path resolves and links through the server, so it
            // can't honor `--dry-run`'s no-write contract. Reject up front,
            // mirroring pnpm's CONFIG_CONFLICT_DRY_RUN_WITH_PNPR_SERVER.
            if dry_run {
                return Err(DryRunIncompatibleWithPnpr.into());
            }
            return Box::pin(install_via_pnpr_inner::<Reporter>(
                &state,
                pnpr_server,
                selection.as_ref(),
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
                    lockfile_path: Some(&lockfile_path),
                    use_state_lockfile: true,
                },
            ))
            .await;
        }

        let install = Install {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            emit_initial_manifest: true,
            lockfile: MaybeLazyLockfile::Lazy(lockfile),
            lockfile_path: Some(&lockfile_path),
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
            installs_only: true,
            resolved_packages,
            supported_architectures,
            node_linker,
            lockfile_only,
            dry_run,
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            auth_override: None,
            resolution_observer: None,
            peer_issues_sink: None,
            catalogs_override: None,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
        };
        match selection.as_ref() {
            Some(selection) => {
                install.run_selected::<Reporter>(workspace_install_selection(selection)).await
            }
            None => install.run::<Reporter>().await,
        }
        .wrap_err("installing dependencies")?;

        Ok(())
    }
}

fn workspace_install_selection(
    selection: &InstallFamilySelection,
) -> WorkspaceInstallSelection<'_> {
    WorkspaceInstallSelection {
        all_projects: &selection.projects,
        ordered_groups: &selection.ordered_groups,
        ordered_dirs: &selection.ordered_dirs,
        selected_dirs: selection.selected_dirs.as_ref(),
        active_manifest_is_standin: selection.active_manifest_is_standin,
    }
}

/// Per-invocation install knobs forwarded to the frozen link pass,
/// already resolved from the CLI flags + config by [`InstallArgs::run`].
pub(crate) struct PnprLink<'a> {
    pub(crate) dependency_groups: Vec<DependencyGroup>,
    pub(crate) supported_architectures:
        Option<pacquet_package_is_installable::SupportedArchitectures>,
    pub(crate) node_linker: NodeLinker,
    pub(crate) skip_runtimes: bool,
    /// Governs the *server's* resolution behavior (frozen vs
    /// reuse-and-update); forwarded to `/-/pnpr/v0/resolve`. The local
    /// materialization always runs frozen against the server-produced
    /// lockfile.
    pub(crate) frozen_lockfile: bool,
    /// The *effective* `preferFrozenLockfile` (the CLI tri-state already
    /// resolved against `config.prefer_frozen_lockfile`, exactly as the
    /// local `Install` resolves it); forwarded to `/-/pnpr/v0/resolve`. `false`
    /// forces the server to re-resolve. Resolving here — rather than
    /// sending the raw CLI override — keeps a yaml `preferFrozenLockfile:
    /// false` honored on the pnpr path without `--no-prefer-frozen-lockfile`.
    pub(crate) prefer_frozen_lockfile: bool,
    /// `--lockfile-only`. Forwarded to `/-/pnpr/v0/resolve` so the server
    /// resolves only — returning the lockfile without fetching files —
    /// after which [`install_via_pnpr`] writes the lockfile and skips
    /// materialization, mirroring pnpm's resolve + write, fetch nothing,
    /// link nothing. See
    /// [pnpm/pnpm#12146](https://github.com/pnpm/pnpm/issues/12146).
    pub(crate) lockfile_only: bool,
    /// `--ignore-manifest-check`; forwarded so the server's frozen
    /// freshness check and the local materialization both skip the
    /// manifest ↔ lockfile comparison.
    pub(crate) ignore_manifest_check: bool,
    /// The effective `trustLockfile` (yaml `trustLockfile` OR
    /// `--trust-lockfile`); forwarded so the server skips verifying the
    /// input lockfile when the user opted out, mirroring the local path.
    pub(crate) trust_lockfile: bool,
    pub(crate) lockfile_path: Option<&'a std::path::Path>,
    pub(crate) use_state_lockfile: bool,
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

fn resolve_project(
    dir: String,
    manifest: &pacquet_package_manifest::PackageManifest,
) -> ResolveProject {
    ResolveProject {
        dir,
        name: manifest.value().get("name").and_then(|value| value.as_str()).map(str::to_string),
        version: manifest
            .value()
            .get("version")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        dependencies: manifest
            .dependencies([DependencyGroup::Prod])
            .map(|(name, spec)| (name.to_string(), spec.to_string()))
            .collect(),
        dev_dependencies: manifest
            .dependencies([DependencyGroup::Dev])
            .map(|(name, spec)| (name.to_string(), spec.to_string()))
            .collect(),
        optional_dependencies: manifest
            .dependencies([DependencyGroup::Optional])
            .map(|(name, spec)| (name.to_string(), spec.to_string()))
            .collect(),
    }
}

/// Resolve the active project or selected workspace projects through a
/// `pnpr` server, then link them.
///
/// Sends the client's registries to the server, which resolves against
/// them and returns the resolved lockfile; writes that lockfile, then
/// runs a frozen install to materialize `node_modules` from it — the
/// frozen install fetches every tarball from the registries itself, like
/// a normal install. Under `--lockfile-only` it stops after writing the
/// lockfile (fetch nothing, link nothing).
pub(crate) async fn install_via_pnpr<Reporter: self::Reporter + 'static>(
    state: &State,
    pnpr_server: &str,
    link: PnprLink<'_>,
) -> miette::Result<()> {
    Box::pin(install_via_pnpr_inner::<Reporter>(state, pnpr_server, None, link)).await
}

async fn install_via_pnpr_inner<Reporter: self::Reporter + 'static>(
    state: &State,
    pnpr_server: &str,
    selection: Option<&InstallFamilySelection>,
    link: PnprLink<'_>,
) -> miette::Result<()> {
    // The pnpr server resolves dependencies and streams missing files
    // straight into the store, so this path inherently writes the store.
    // `frozenStore` promises the store is complete and read-only, so the
    // two are mutually exclusive — refuse up front instead of failing on
    // the read-only write with the `FROZEN_STORE_INCOMPATIBLE_WITH_PNPR`
    // guard.
    if state.config.frozen_store {
        return Err(FrozenStoreIncompatibleWithPnpr.into());
    }

    let previous_wanted = if link.use_state_lockfile {
        state
            .lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?
            .cloned()
    } else {
        None
    };
    let selection_importer_ids = selection.map(|selection| {
        let real_importer_ids = selection
            .projects
            .iter()
            .map(|project| {
                pacquet_workspace::importer_id_from_root_dir(
                    &selection.workspace_root,
                    &project.root_dir,
                )
            })
            .collect();
        let selected_importer_ids = selection
            .selected_dirs
            .iter()
            .map(|project_dir| {
                pacquet_workspace::importer_id_from_root_dir(&selection.workspace_root, project_dir)
            })
            .collect();
        (real_importer_ids, selected_importer_ids)
    });
    let partial_selection = selection_importer_ids.as_ref().is_some_and(
        |(real_importer_ids, selected_importer_ids)| real_importer_ids != selected_importer_ids,
    );
    let projects = resolve_projects_for_pnpr(state, selection, link.use_state_lockfile)?;
    let full_workspace_importer_ids = (selection.is_none()
        && link.use_state_lockfile
        && state.config.shared_workspace_lockfile
        && state.config.workspace_dir.is_some())
    .then(|| {
        let importer_ids: std::collections::HashSet<_> =
            projects.iter().map(|project| project.dir.clone()).collect();
        (importer_ids.clone(), importer_ids)
    });

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
    let opts = ResolveProjectsOptions {
        projects,
        registry: resolve_registry,
        named_registries: state.config.named_registries.clone(),
        // Only the caller's identity to pnpr is sent. Upstream registry
        // credentials are never forwarded: pnpr selects them from its own
        // route policy, so they stay out of the request body.
        authorization: state.config.auth_headers.for_url(pnpr_server),
        overrides,
        lockfile: previous_wanted.clone(),
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
    let lockfile_dir = link.lockfile_path.and_then(|path| path.parent()).unwrap_or_else(|| {
        state.manifest.path().parent().expect("manifest path always has a parent dir")
    });
    let lockfile_path = link
        .lockfile_path
        .map_or_else(|| lockfile_dir.join(Lockfile::FILE_NAME), std::path::Path::to_path_buf);

    if link.frozen_lockfile
        && (selection.is_some() || !link.lockfile_only)
        && let Some(lockfile) = previous_wanted.as_ref()
    {
        let prefetcher = if link.lockfile_only {
            None
        } else {
            let selected_prefetch_lockfile =
                selection_importer_ids.as_ref().map(|(_, selected_importer_ids)| {
                    let hoisted_importer_ids = matches!(link.node_linker, NodeLinker::Hoisted)
                        .then(|| {
                            lockfile
                                .importers
                                .keys()
                                .cloned()
                                .collect::<std::collections::HashSet<_>>()
                        });
                    let initial_importer_ids =
                        hoisted_importer_ids.as_ref().unwrap_or(selected_importer_ids);
                    materialization_closure(
                        lockfile,
                        lockfile_dir,
                        initial_importer_ids,
                        IncludedDependencies {
                            dependencies: link.dependency_groups.contains(&DependencyGroup::Prod),
                            dev_dependencies: link
                                .dependency_groups
                                .contains(&DependencyGroup::Dev),
                            optional_dependencies: link
                                .dependency_groups
                                .contains(&DependencyGroup::Optional),
                        },
                        &SkippedSnapshots::new(),
                    )
                    .lockfile
                });
            let prefetcher = TarballPrefetcher::new(
                state.config,
                &state.http_client,
                &state.tarball_mem_cache,
                None,
                &lockfile_dir.to_string_lossy(),
            )
            .await;
            prefetcher
                .prefetch_lockfile(
                    selected_prefetch_lockfile.as_ref().unwrap_or(lockfile),
                    state.config,
                )
                .await;
            tokio::task::yield_now().await;
            Some(prefetcher)
        };

        let lockfile_verification_override: Option<LockfileVerificationOverride<'_>> =
            if link.trust_lockfile {
                None
            } else {
                let verify_opts = VerifyLockfileOptions::from_resolve_projects_options(&opts)
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
            emit_initial_manifest: true,
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
            installs_only: true,
            resolved_packages: &state.resolved_packages,
            supported_architectures: link.supported_architectures,
            node_linker: link.node_linker,
            lockfile_only: link.lockfile_only,
            dry_run: false,
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            auth_override: None,
            resolution_observer: None,
            peer_issues_sink: None,
            catalogs_override: None,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
        };

        let result = match (selection, lockfile_verification_override) {
            (Some(selection), Some(lockfile_verification_override)) => {
                Box::pin(install.run_selected_with_lockfile_verification::<Reporter>(
                    workspace_install_selection(selection),
                    lockfile_verification_override,
                ))
                .await
            }
            (Some(selection), None) => {
                Box::pin(install.run_selected::<Reporter>(workspace_install_selection(selection)))
                    .await
            }
            (None, Some(lockfile_verification_override)) => {
                install
                    .run_with_lockfile_verification::<Reporter>(lockfile_verification_override)
                    .await
            }
            (None, None) => install.run::<Reporter>().await,
        };
        // On failure the prefetcher is dropped, not shut down: shutdown
        // waits for every in-flight prefetch download (each task holds a
        // store-index writer handle), which would hold the fail-fast
        // abort hostage to the remaining transfers. The index rows are
        // best-effort — a dropped row only costs a later re-download.
        result.wrap_err("restoring dependencies from the local lockfile via pnpr verification")?;

        if let Some(prefetcher) = prefetcher {
            prefetcher.shutdown().await;
        }

        return Ok(());
    }

    // Under `--lockfile-only` nothing is materialized, so skip the
    // prefetcher entirely and consume the stream with a no-op callback.
    // A partial install also waits for the merged lockfile before fetching,
    // because only then is the selected workspace closure known. Otherwise
    // spawn a prefetcher that fires each tarball download as its `package`
    // frame streams in, so fetch overlaps the server's resolution
    // ([pnpm/pnpm#12234](https://github.com/pnpm/pnpm/issues/12234)); the
    // frozen materialization install below then finds every tarball already
    // in the shared mem cache.
    let prefetcher = if link.lockfile_only || partial_selection {
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
                .resolve_projects_streaming(opts, |pkg| {
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
        None => client.resolve_projects(opts).await,
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
    if let (Some((real_importer_ids, selected_importer_ids)), Some(workspace_root)) = (
        selection_importer_ids.as_ref().or(full_workspace_importer_ids.as_ref()),
        selection
            .map(|selection| selection.workspace_root.as_path())
            .or(state.config.workspace_dir.as_deref()),
    ) {
        outcome.lockfile = merge_filtered_wanted_lockfile(
            previous_wanted.as_ref(),
            outcome.lockfile,
            real_importer_ids,
            selected_importer_ids,
            workspace_root,
        )
        .map_err(miette::Report::new)?;
    }

    if state.config.lockfile {
        outcome
            .lockfile
            .save_to_path(&lockfile_path)
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

    let install = Install {
        tarball_mem_cache: std::sync::Arc::clone(&state.tarball_mem_cache),
        http_client: &state.http_client,
        http_client_arc: std::sync::Arc::clone(&state.http_client),
        config: state.config,
        manifest: &state.manifest,
        emit_initial_manifest: true,
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
        installs_only: true,
        resolved_packages: &state.resolved_packages,
        supported_architectures: link.supported_architectures,
        node_linker: link.node_linker,
        lockfile_only: false,
        dry_run: false,
        update_seed_policy: UpdateSeedPolicy::KeepAll,
        auth_override: None,
        resolution_observer: None,
        peer_issues_sink: None,
        catalogs_override: None,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    };
    match selection {
        Some(selection) => {
            Box::pin(install.run_selected::<Reporter>(workspace_install_selection(selection))).await
        }
        None => install.run::<Reporter>().await,
    }
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

fn resolve_projects_for_pnpr(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    use_state_lockfile: bool,
) -> miette::Result<Vec<ResolveProject>> {
    if let Some(selection) = selection {
        return Ok(resolve_workspace_projects(&selection.workspace_root, &selection.projects));
    }
    if use_state_lockfile
        && state.config.shared_workspace_lockfile
        && let Some(workspace_root) = state.config.workspace_dir.as_deref()
    {
        let (projects, _) = discover_workspace_projects(workspace_root)?;
        return Ok(resolve_workspace_projects(workspace_root, &projects));
    }
    Ok(vec![resolve_project(".".to_string(), &state.manifest)])
}

fn resolve_workspace_projects(
    workspace_root: &std::path::Path,
    projects: &[pacquet_workspace::Project],
) -> Vec<ResolveProject> {
    projects
        .iter()
        .map(|project| {
            resolve_project(
                pacquet_workspace::importer_id_from_root_dir(workspace_root, &project.root_dir),
                &project.manifest,
            )
        })
        .collect()
}

struct PnprBenchmarkRegistryOverride {
    resolve_registry: String,
    tarball_rewrite: Option<BenchmarkRegistryRewrite>,
}

impl PnprBenchmarkRegistryOverride {
    /// Benchmark-only hook for `pnpm/tasks/integrated-benchmark`.
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
        // Custom resolutions are opaque — the benchmark rewrite can't
        // know which of their fields (if any) is a registry URL.
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Registry(_)
        | LockfileResolution::Custom(_) => {}
    }
}

#[cfg(test)]
mod tests;
