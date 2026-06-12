use crate::{State, cli_args::supported_architectures::SupportedArchitecturesArgs, config_deps};
use clap::Args;
use miette::Context;
use pacquet_package_manager::Add;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use std::path::{Path, PathBuf};

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
    fn save_prod(&self) -> bool {
        let &AddDependencyOptions { save_prod, save_dev, save_optional, save_peer } = self;
        save_prod || (!save_dev && !save_optional && !save_peer)
    }

    /// Whether to add entry to `"devDependencies"`.
    ///
    /// **NOTE:** `--save-peer` without any other `--save-*` flags implies save as dev.
    fn save_dev(&self) -> bool {
        let &AddDependencyOptions { save_prod, save_dev, save_optional, save_peer } = self;
        save_dev || (!save_prod && !save_optional && save_peer)
    }

    /// Whether to add entry to `"optionalDependencies"`.
    fn save_optional(&self) -> bool {
        self.save_optional
    }

    /// Whether to add entry to `"peerDependencies"`.
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
    /// Save the new dependency to the default catalog: `catalog:` is written
    /// to `package.json` and the specifier to `pnpm-workspace.yaml`'s
    /// `catalog:` block. Shorthand for `--save-catalog-name=default`.
    #[clap(long = "save-catalog")]
    pub save_catalog: bool,
    /// Save the new dependency to the named catalog `<name>`: `catalog:<name>`
    /// is written to `package.json` and the specifier to the matching entry
    /// under `pnpm-workspace.yaml`'s `catalogs:`.
    #[clap(long = "save-catalog-name", value_name = "name")]
    pub save_catalog_name: Option<String>,
    /// Add the package as a configurational dependency: the clean
    /// specifier is written to `pnpm-workspace.yaml`'s `configDependencies`
    /// block, the resolved version + integrity to the env lockfile, and
    /// the package linked into `node_modules/.pnpm-config`. Mirrors pnpm's
    /// `pnpm add --config`.
    #[clap(long = "config")]
    pub config: bool,
    /// Dependencies are not downloaded. The package is added to the
    /// manifest and only `pnpm-lock.yaml` is updated; no `node_modules`
    /// is created. Mirrors pnpm's `--lockfile-only`.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,
    /// The directory with links to the store (default is `node_modules/.pacquet`).
    /// All direct and indirect dependencies of the project are linked into this directory
    #[clap(long = "virtual-store-dir", default_value = "node_modules/.pacquet")]
    pub virtual_store_dir: Option<PathBuf>, // TODO: make use of this
}

impl AddArgs {
    /// Execute the subcommand.
    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        // `--config` routes to the configurational-dependency path
        // instead of the regular `package.json` add: resolve + install
        // into `.pnpm-config`, then record the clean specifier in
        // `pnpm-workspace.yaml`.
        if self.config {
            let parsed = parse_wanted_dependency(&self.package_name);
            let Some(name) = parsed.alias else {
                return Err(miette::miette!(
                    "'{}' is not a valid package name for a configuration dependency",
                    self.package_name,
                ));
            };
            // No version given → resolve the `latest` tag, matching the
            // default `add` behavior.
            let specifier = parsed.bare_specifier.unwrap_or_else(|| "latest".to_string());
            // configDependencies are workspace-level: write to the
            // workspace root's `pnpm-workspace.yaml` / env lockfile /
            // `.pnpm-config`, not the current package's. Fall back to the
            // manifest's directory for a single-package repo.
            let root_dir = state.config.workspace_dir.clone().unwrap_or_else(|| {
                state.manifest.path().parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf)
            });
            return config_deps::add_config_dependency::<Reporter>(
                state.config,
                &root_dir,
                &name,
                &specifier,
            )
            .await;
        }

        // Merge CLI overrides with the yaml-derived value before
        // handing off to the install pipeline. See
        // `cli_args::install.rs` for the parallel comment — the
        // pattern is identical (clone from `&'static Config`, merge,
        // pass merged value through).
        let supported_architectures =
            self.supported_architectures.apply_to(state.config.supported_architectures.clone());

        // `--save-catalog-name=<name>` wins; `--save-catalog` is the
        // shorthand for the default catalog; otherwise fall back to the
        // `saveCatalogName` config default (`None`). Mirrors pnpm's
        // `save-catalog` → `--save-catalog-name=default` shorthand.
        let save_catalog_name = self
            .save_catalog_name
            .clone()
            .or_else(|| self.save_catalog.then(|| "default".to_string()))
            .or_else(|| state.config.save_catalog_name.clone());

        add_package::<Reporter, _, _>(
            state,
            &self.package_name,
            self.save_exact,
            save_catalog_name,
            self.lockfile_only,
            supported_architectures,
            || self.dependency_options.dependency_groups(),
        )
        .await
    }
}

/// Add a single package to `state`'s manifest and install it.
///
/// Shared by `pacquet add` and `pacquet dlx`. dlx points `state` at a
/// cache directory (via a [`Config`](pacquet_config::Config) whose
/// `modules_dir` is anchored there) and saves to `dependencies` so the
/// package's bin lands in `<cacheDir>/node_modules/.bin`.
pub(crate) async fn add_package<Reporter, ListDependencyGroups, DependencyGroupList>(
    mut state: State,
    package_name: &str,
    save_exact: bool,
    save_catalog_name: Option<String>,
    lockfile_only: bool,
    supported_architectures: Option<pacquet_package_is_installable::SupportedArchitectures>,
    list_dependency_groups: ListDependencyGroups,
) -> miette::Result<()>
where
    Reporter: self::Reporter + 'static,
    ListDependencyGroups: Fn() -> DependencyGroupList,
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    // TODO: if a package already exists in another dependency group, don't remove the existing entry.
    let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
        &mut state;

    let lockfile_path =
        manifest.path().parent().map(|parent| parent.join(pacquet_lockfile::Lockfile::FILE_NAME));
    Add {
        tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
        http_client,
        http_client_arc: std::sync::Arc::clone(http_client),
        config,
        manifest,
        lockfile: lockfile.as_ref(),
        lockfile_path: lockfile_path.as_deref(),
        list_dependency_groups,
        package_name,
        save_exact,
        save_catalog_name,
        resolved_packages,
        supported_architectures,
        lockfile_only,
    }
    .run::<Reporter>()
    .await
    .wrap_err("adding a new package")
}

#[cfg(test)]
mod tests;
