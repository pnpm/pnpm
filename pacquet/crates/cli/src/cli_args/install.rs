use crate::State;
use crate::cli_args::supported_architectures::SupportedArchitecturesArgs;
use clap::{Args, ValueEnum};
use miette::Context;
use pacquet_config::NodeLinker;
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
}

impl InstallArgs {
    pub async fn run<Reporter: self::Reporter>(self, state: State) -> miette::Result<()> {
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;
        let InstallArgs {
            dependency_options,
            supported_architectures,
            frozen_lockfile,
            no_runtime,
            node_linker,
            offline: _,
            prefer_offline: _,
        } = self;

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

        // `--node-linker` flag (if passed) overrides the
        // yaml/npmrc value for this invocation. Mirrors pnpm's
        // override-on-explicit-flag semantics.
        let node_linker = node_linker.map(NodeLinkerArg::into_config).unwrap_or(config.node_linker);
        Install {
            tarball_mem_cache,
            http_client,
            config,
            manifest,
            lockfile: lockfile.as_ref(),
            dependency_groups: dependency_options.dependency_groups(),
            frozen_lockfile,
            skip_runtimes,
            resolved_packages,
            supported_architectures,
            node_linker,
        }
        .run::<Reporter>()
        .await
        .wrap_err("installing dependencies")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests;
