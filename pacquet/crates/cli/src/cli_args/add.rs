use crate::State;
use crate::cli_args::supported_architectures::SupportedArchitecturesArgs;
use clap::Args;
use miette::Context;
use pacquet_package_manager::Add;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct AddDependencyOptions {
    /// Install the specified packages as regular dependencies.
    #[clap(short = 'P', long)]
    save_prod: bool,
    /// Install the specified packages as devDependencies.
    #[clap(short = 'D', long)]
    save_dev: bool,
    /// Install the specified packages as optionalDependencies.
    #[clap(short = 'O', long)]
    save_optional: bool,
    /// Using --save-peer will add one or more packages to peerDependencies and install them as dev dependencies
    #[clap(long)]
    save_peer: bool,
}

impl AddDependencyOptions {
    /// Whether to add entry to `"dependencies"`.
    ///
    /// **NOTE:** no `--save-*` flags implies save as prod.
    #[inline(always)]
    fn save_prod(&self) -> bool {
        let &AddDependencyOptions { save_prod, save_dev, save_optional, save_peer } = self;
        save_prod || (!save_dev && !save_optional && !save_peer)
    }

    /// Whether to add entry to `"devDependencies"`.
    ///
    /// **NOTE:** `--save-peer` without any other `--save-*` flags implies save as dev.
    #[inline(always)]
    fn save_dev(&self) -> bool {
        let &AddDependencyOptions { save_prod, save_dev, save_optional, save_peer } = self;
        save_dev || (!save_prod && !save_optional && save_peer)
    }

    /// Whether to add entry to `"optionalDependencies"`.
    #[inline(always)]
    fn save_optional(&self) -> bool {
        self.save_optional
    }

    /// Whether to add entry to `"peerDependencies"`.
    #[inline(always)]
    fn save_peer(&self) -> bool {
        self.save_peer
    }

    /// Convert the `--save-*` flags to an iterator of [`DependencyGroup`]
    /// which selects which target group to save to.
    fn dependency_groups(&self) -> impl Iterator<Item = DependencyGroup> {
        std::iter::empty()
            .chain(self.save_prod().then_some(DependencyGroup::Prod))
            .chain(self.save_dev().then_some(DependencyGroup::Dev))
            .chain(self.save_optional().then_some(DependencyGroup::Optional))
            .chain(self.save_peer().then_some(DependencyGroup::Peer))
    }
}

#[derive(Debug, Args)]
pub struct AddArgs {
    /// Name of the package
    pub package_name: String, // TODO: 1. support version range, 2. multiple arguments, 3. name this `packages`
    /// --save-prod, --save-dev, --save-optional, --save-peer
    #[clap(flatten)]
    pub dependency_options: AddDependencyOptions,
    /// `--cpu` / `--os` / `--libc` overrides for the optional-dep
    /// platform filter. Mirrors upstream pnpm's CLI flags; merges
    /// per-axis into `supportedArchitectures` loaded from
    /// `pnpm-workspace.yaml`.
    #[clap(flatten)]
    pub supported_architectures: SupportedArchitecturesArgs,
    /// Saved dependencies will be configured with an exact version rather than using
    /// the default semver range operator.
    #[clap(short = 'E', long = "save-exact")]
    pub save_exact: bool,
    /// The directory with links to the store (default is node_modules/.pacquet).
    /// All direct and indirect dependencies of the project are linked into this directory
    #[clap(long = "virtual-store-dir", default_value = "node_modules/.pacquet")]
    pub virtual_store_dir: Option<PathBuf>, // TODO: make use of this
}

impl AddArgs {
    /// Execute the subcommand.
    pub async fn run<Reporter: self::Reporter>(self, mut state: State) -> miette::Result<()> {
        // TODO: if a package already exists in another dependency group, don't remove the existing entry.

        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &mut state;

        // Merge CLI overrides with the yaml-derived value before
        // handing off to the install pipeline. See
        // `cli_args::install.rs` for the parallel comment — the
        // pattern is identical (clone from `&'static Config`, merge,
        // pass merged value through).
        let supported_architectures =
            self.supported_architectures.apply_to(config.supported_architectures.clone());

        Add {
            tarball_mem_cache,
            http_client,
            config,
            manifest,
            lockfile: lockfile.as_ref(),
            list_dependency_groups: || self.dependency_options.dependency_groups(),
            package_name: &self.package_name,
            save_exact: self.save_exact,
            resolved_packages,
            supported_architectures,
        }
        .run::<Reporter>()
        .await
        .wrap_err("adding a new package")
    }
}

#[cfg(test)]
mod tests;
