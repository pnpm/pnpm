use crate::{State, cli_args::supported_architectures::SupportedArchitecturesArgs};
use clap::{Args, ValueEnum};
use miette::Context;
use pacquet_agent_client::{AgentClient, InstallOptions as AgentInstallOptions};
use pacquet_config::{NodeLinker, TrustPolicy};
use pacquet_lockfile::Lockfile;
use pacquet_package_manager::Install;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;

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
    /// they were already installed, if the NODE_ENV environment variable is set to production.
    /// Use this flag to instruct pacquet to ignore NODE_ENV and take its production status from this
    /// flag instead.
    #[arg(short = 'P', long)]
    prod: bool,
    /// Only devDependencies are installed and dependencies are removed insofar they were
    /// already installed, regardless of the NODE_ENV.
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
    /// --prod, --dev, and --no-optional
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
    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;
        let InstallArgs {
            dependency_options,
            supported_architectures,
            frozen_lockfile,
            lockfile_only,
            prefer_frozen_lockfile,
            no_prefer_frozen_lockfile,
            ignore_manifest_check,
            no_runtime,
            node_linker,
            offline: _,
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
        let node_linker = node_linker.map(NodeLinkerArg::into_config).unwrap_or(config.node_linker);
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
            return install_via_pnpr::<Reporter>(
                &state,
                pnpr_server,
                AgentLink {
                    dependency_groups: dependency_options.dependency_groups().collect(),
                    supported_architectures,
                    node_linker,
                    skip_runtimes,
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
            lockfile: lockfile.as_ref(),
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
struct AgentLink<'a> {
    dependency_groups: Vec<DependencyGroup>,
    supported_architectures: Option<pacquet_package_is_installable::SupportedArchitectures>,
    node_linker: NodeLinker,
    skip_runtimes: bool,
    trust_lockfile: bool,
    lockfile_path: Option<&'a std::path::Path>,
}

/// Resolve a single project through a `pnpr` server, then link it.
///
/// Sends the client's registries to the server, which resolves against
/// them and streams back the missing files; writes the server-produced
/// lockfile, then runs a frozen install to materialize `node_modules`
/// from it — the equivalent of pnpm's `installFromPnpmRegistry` handing
/// off to `headlessInstall`.
async fn install_via_pnpr<Reporter: self::Reporter + 'static>(
    state: &State,
    pnpr_server: &str,
    link: AgentLink<'_>,
) -> miette::Result<()> {
    // The server resolves remotely, so the local resolver-side
    // `trustPolicy: no-downgrade` check can't run. Refuse rather than
    // silently link a lockfile the local verifier would reject — mirrors
    // pnpm's `TRUST_POLICY_INCOMPATIBLE_WITH_AGENT` guard.
    if state.config.trust_policy == TrustPolicy::NoDowngrade {
        return Err(miette::miette!(
            "A pnprServer does not enforce `trustPolicy: no-downgrade`; unset it or unset pnprServer so resolution runs locally."
        ));
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

    let overrides =
        state.config.overrides.as_ref().and_then(|overrides| serde_json::to_value(overrides).ok());

    let outcome = AgentClient::new(pnpr_server)
        .install(AgentInstallOptions {
            store_dir: &state.config.store_dir,
            dependencies,
            dev_dependencies,
            registry: state.config.registry.clone(),
            named_registries: state.config.named_registries.clone(),
            overrides,
            minimum_release_age: state.config.minimum_release_age,
        })
        .await
        .map_err(|err| miette::miette!("{err}"))
        .wrap_err("resolving dependencies via the pnpr server")?;

    if state.config.lockfile {
        let lockfile_dir =
            state.manifest.path().parent().expect("manifest path always has a parent dir");
        outcome
            .lockfile
            .save_to_path(&lockfile_dir.join(Lockfile::FILE_NAME))
            .map_err(|err| miette::miette!("{err}"))
            .wrap_err("writing the agent-resolved lockfile")?;
    }

    Install {
        tarball_mem_cache: std::sync::Arc::clone(&state.tarball_mem_cache),
        http_client: &state.http_client,
        http_client_arc: std::sync::Arc::clone(&state.http_client),
        config: state.config,
        manifest: &state.manifest,
        lockfile: Some(&outcome.lockfile),
        lockfile_path: link.lockfile_path,
        dependency_groups: link.dependency_groups,
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: link.skip_runtimes,
        trust_lockfile: link.trust_lockfile,
        update_checksums: false,
        is_full_install: true,
        resolved_packages: &state.resolved_packages,
        supported_architectures: link.supported_architectures,
        node_linker: link.node_linker,
        lockfile_only: false,
    }
    .run::<Reporter>()
    .await
    .wrap_err("linking dependencies resolved via the pnpr server")?;

    Ok(())
}

#[cfg(test)]
mod tests;
