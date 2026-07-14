use crate::{
    State,
    cli_args::{
        install::resolve_bool_override, supported_architectures::SupportedArchitecturesArgs,
    },
    config_deps,
};
use clap::Args;
use miette::Context;
use pacquet_config::Config;
use pacquet_package_manager::Add;
use pacquet_package_manifest::DependencyGroup;
use pacquet_registry::PinnedVersion;
use pacquet_reporter::Reporter;
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

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
    fn save_prod(&self) -> bool {
        let &AddDependencyOptions { save_prod, save_dev, save_optional, save_peer } = self;
        save_prod || (!save_dev && !save_optional && !save_peer)
    }

    /// Whether to add entry to `"devDependencies"`.
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
    /// Names of the packages to add.
    #[clap(required = true)]
    pub package_names: Vec<String>,
    /// --save-prod, --save-dev, --save-optional, --save-peer
    #[clap(flatten)]
    pub dependency_options: AddDependencyOptions,
    /// `--cpu`, `--os`, and `--libc` filters for which optional dependencies are installed.
    #[clap(flatten)]
    pub supported_architectures: SupportedArchitecturesArgs,
    /// Saved dependencies will be configured with an exact version rather than using
    /// the default semver range operator.
    #[clap(short = 'E', long = "save-exact")]
    pub save_exact: bool,
    /// The prefix of the saved version range: `^` (default), `~`, or empty for an exact version.
    #[clap(long = "save-prefix", value_name = "prefix")]
    pub save_prefix: Option<String>,
    /// Save the new dependency to the default catalog. Shorthand for `--save-catalog-name=default`.
    #[clap(long = "save-catalog")]
    pub save_catalog: bool,
    /// Save the new dependency to the named catalog `<name>`.
    #[clap(long = "save-catalog-name", value_name = "name")]
    pub save_catalog_name: Option<String>,
    /// Add the package as a configuration dependency.
    #[clap(long = "config")]
    pub config: bool,
    /// Dependencies are not downloaded. Only `pnpm-lock.yaml` is updated.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,
    /// The directory with links to the store (default is `node_modules/.pacquet`).
    /// All direct and indirect dependencies of the project are linked into this directory
    #[clap(long = "virtual-store-dir", default_value = "node_modules/.pacquet")]
    pub virtual_store_dir: Option<PathBuf>, // TODO: make use of this

    /// Install the package globally, linking its bins into the global bin directory.
    #[clap(short = 'g', long)]
    pub global: bool,
    /// Don't run lifecycle scripts of the added package or its dependencies.
    #[clap(long = "ignore-scripts", overrides_with = "no_ignore_scripts")]
    pub ignore_scripts: bool,
    /// Force-enable lifecycle scripts for this invocation.
    #[clap(long = "no-ignore-scripts", overrides_with = "ignore_scripts")]
    pub no_ignore_scripts: bool,
}

impl AddArgs {
    pub(crate) fn apply_cli_config(&self, config: &mut Config) {
        config.ignore_scripts = resolve_bool_override(
            self.ignore_scripts,
            self.no_ignore_scripts,
            config.ignore_scripts,
        );
    }

    pub(super) fn parse_config_dependencies(
        &self,
    ) -> miette::Result<Option<BTreeMap<String, String>>> {
        if !self.config {
            return Ok(None);
        }

        let mut added = BTreeMap::new();
        for package_name in &self.package_names {
            let parsed = parse_wanted_dependency(package_name);
            let Some(name) = parsed.alias else {
                return Err(miette::miette!(
                    "'{package_name}' is not a valid package name for a configuration dependency",
                ));
            };
            let specifier = parsed.bare_specifier.unwrap_or_else(|| "latest".to_string());
            added.insert(name, specifier);
        }
        Ok(Some(added))
    }

    /// Execute the subcommand.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        config_dependencies: Option<BTreeMap<String, String>>,
    ) -> miette::Result<()> {
        // `--config` routes to the configurational-dependency path
        // instead of the regular `package.json` add: resolve + install
        // into `.pnpm-config`, then record the clean specifier in
        // `pnpm-workspace.yaml`.
        if self.config {
            let added = config_dependencies
                .expect("config dependency selectors are parsed before state initialization");
            // configDependencies are workspace-level: write to the
            // workspace root's `pnpm-workspace.yaml` / env lockfile /
            // `.pnpm-config`, not the current package's. Fall back to the
            // manifest's directory for a single-package repo.
            let root_dir = state.config.workspace_dir.clone().unwrap_or_else(|| {
                state.manifest.path().parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf)
            });
            return config_deps::add_config_dependencies::<Reporter>(
                state.config,
                &root_dir,
                &added,
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

        // Collapse the `--save-exact` / `--save-prefix` flags into the pinned
        // version that decides the saved range, mirroring pnpm's
        // `getPinnedVersion`.
        let pinned_version =
            PinnedVersion::from_save_options(self.save_exact, self.save_prefix.as_deref());

        add_packages::<Reporter, _, _>(
            state,
            &self.package_names,
            pinned_version,
            save_catalog_name,
            self.lockfile_only,
            supported_architectures,
            || self.dependency_options.dependency_groups(),
        )
        .await
    }

    /// `pnpm add -g`: install the package into the global packages
    /// directory and link its bins. Delegates to
    /// [`crate::cli_args::global::handle_global_add`].
    pub async fn run_global<Reporter: self::Reporter + 'static>(
        self,
        config: &'static Config,
        dir: &Path,
    ) -> miette::Result<()> {
        // `--config` (configurational dependency) and `--lockfile-only` have
        // no meaning for a global install; reject rather than silently ignore.
        if self.config {
            return Err(miette::miette!(
                "`pacquet add --config` cannot be combined with --global."
            ));
        }
        if self.lockfile_only {
            return Err(miette::miette!(
                "`pacquet add --lockfile-only` cannot be combined with --global."
            ));
        }
        let supported_architectures =
            self.supported_architectures.apply_to(config.supported_architectures.clone());
        let pinned_version =
            PinnedVersion::from_save_options(self.save_exact, self.save_prefix.as_deref());
        Box::pin(crate::cli_args::global::handle_global_add::<Reporter>(
            config,
            &self.package_names,
            pinned_version,
            supported_architectures,
            dir,
        ))
        .await
    }
}

/// Add a single package to `state`'s manifest and install it.
///
/// Compatibility adapter for single-package callers such as `pacquet dlx`.
pub(crate) async fn add_package<Reporter, ListDependencyGroups, DependencyGroupList>(
    state: State,
    package_name: &str,
    pinned_version: PinnedVersion,
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
    let package_names = [package_name.to_string()];
    Box::pin(add_packages::<Reporter, _, _>(
        state,
        &package_names,
        pinned_version,
        save_catalog_name,
        lockfile_only,
        supported_architectures,
        list_dependency_groups,
    ))
    .await
}

/// Add packages to `state`'s manifest and install them in one operation.
pub(crate) async fn add_packages<Reporter, ListDependencyGroups, DependencyGroupList>(
    mut state: State,
    package_names: &[String],
    pinned_version: PinnedVersion,
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
    let lockfile =
        lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

    let lockfile_path =
        manifest.path().parent().map(|parent| parent.join(pacquet_lockfile::Lockfile::FILE_NAME));
    Add {
        tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
        http_client,
        http_client_arc: std::sync::Arc::clone(http_client),
        config,
        manifest,
        lockfile,
        lockfile_path: lockfile_path.as_deref(),
        list_dependency_groups,
        package_names,
        pinned_version,
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
