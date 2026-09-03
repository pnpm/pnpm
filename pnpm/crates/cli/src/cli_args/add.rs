use crate::{
    State,
    cli_args::{
        install::resolve_bool_override, lockfile_dir::LockfileDirArg,
        pipelines::InstallFamilySelection, supported_architectures::SupportedArchitecturesArgs,
    },
    config_deps,
    engine_pm::{
        error::EngineError,
        pin::{
            declared_package_manager, describe_pin, record_package_manager_pin, resolve_project_pin,
        },
        selector::tool_install_selector,
    },
};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_package_manager::Add;
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use pnpm_workspace_manifest_writer::set_allow_builds;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Args)]
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
    #[clap(long, overrides_with = "no_save_peer")]
    save_peer: bool,
    /// Don't add the packages to peerDependencies, overriding a
    /// `savePeer: true` setting.
    #[clap(long = "no-save-peer", overrides_with = "save_peer")]
    no_save_peer: bool,
}

impl AddDependencyOptions {
    /// `--save-peer` / `--no-save-peer` layered over the `savePeer` setting.
    fn with_save_peer_setting(self, save_peer: bool) -> Self {
        Self {
            save_peer: resolve_bool_override(self.save_peer, self.no_save_peer, save_peer),
            ..self
        }
    }

    /// Whether to add entry to `"dependencies"`.
    fn save_prod(&self) -> bool {
        let &AddDependencyOptions {
            save_prod,
            save_dev,
            save_optional,
            save_peer,
            no_save_peer: _,
        } = self;
        save_prod || (!save_dev && !save_optional && !save_peer)
    }

    /// Whether to add entry to `"devDependencies"`.
    fn save_dev(&self) -> bool {
        let &AddDependencyOptions {
            save_prod,
            save_dev,
            save_optional,
            save_peer,
            no_save_peer: _,
        } = self;
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

    /// The save target for the install layer: `Some` when a `--save-*`
    /// flag names it explicitly, `None` when pnpm infers it per package
    /// (an already-declared dependency is updated in the group it
    /// occupies; a new one lands in `dependencies`).
    fn save_target(&self) -> Option<Vec<DependencyGroup>> {
        let &AddDependencyOptions {
            save_prod,
            save_dev,
            save_optional,
            save_peer,
            no_save_peer: _,
        } = self;
        (save_prod || save_dev || save_optional || save_peer)
            .then(|| self.dependency_groups().collect())
    }
}

#[derive(Debug, Clone, Args)]
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
    /// The prefix of the saved version range: `^` (default), `~`, `=` for an explicit exact pin, or empty for a bare exact version.
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
    /// Package names allowed to run lifecycle (build) scripts during this
    /// install, appended to `allowBuilds`. May be repeated.
    #[clap(long = "allow-build")]
    pub allow_build: Vec<String>,
    /// Dependencies are not downloaded. Only `pnpm-lock.yaml` is updated.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,
    #[clap(flatten)]
    pub lockfile_dir: LockfileDirArg,
    /// Install the package globally, linking its bins into the global bin directory.
    #[clap(short = 'g', long)]
    pub global: bool,
    /// Don't run lifecycle scripts of the added package or its dependencies.
    #[clap(long = "ignore-scripts", overrides_with = "no_ignore_scripts")]
    pub ignore_scripts: bool,
    /// Force-enable lifecycle scripts for this invocation.
    #[clap(long = "no-ignore-scripts", overrides_with = "ignore_scripts")]
    pub no_ignore_scripts: bool,
    /// Permit adding dependencies to a multi-package workspace root without `-w`.
    #[clap(
        long = "ignore-workspace-root-check",
        overrides_with = "no_ignore_workspace_root_check"
    )]
    pub ignore_workspace_root_check: bool,
    /// Keep the workspace-root safety check enabled.
    #[clap(
        long = "no-ignore-workspace-root-check",
        hide = true,
        overrides_with = "ignore_workspace_root_check"
    )]
    pub no_ignore_workspace_root_check: bool,
    /// Include optionalDependencies while materializing the updated project.
    #[clap(long, overrides_with = "no_optional")]
    pub optional: bool,
    /// Exclude optionalDependencies while materializing the updated project.
    #[clap(long = "no-optional", overrides_with = "optional")]
    pub no_optional: bool,
    /// Disable pnpm hooks defined in `.pnpmfile.cjs`, including the
    /// pnpmfiles of config dependencies.
    #[clap(long = "ignore-pnpmfile")]
    pub ignore_pnpmfile: bool,
    /// Reinstall every package the lockfile names: relink packages an
    /// earlier install already materialized, and install optional
    /// dependencies whose `cpu` / `os` / `libc` / `engines` don't match
    /// the host instead of skipping them.
    #[clap(long)]
    pub force: bool,
}

impl AddArgs {
    pub(crate) fn check_workspace_root(&self, config: &Config, dir: &Path) -> miette::Result<()> {
        if config.recursive
            || config.workspace_root
            || resolve_bool_override(
                self.ignore_workspace_root_check,
                self.no_ignore_workspace_root_check,
                config.ignore_workspace_root_check,
            )
            || config.workspace_dir.as_deref() != Some(dir)
        {
            return Ok(());
        }
        let patterns = pnpm_workspace::read_workspace_manifest(dir)
            .into_diagnostic()?
            .map(|manifest| pnpm_workspace::workspace_package_patterns(&manifest));
        if patterns.as_ref().is_some_and(|patterns| patterns.len() > 1) {
            return Err(AddError::AddingToRoot.into());
        }
        Ok(())
    }

    pub(crate) fn apply_cli_config(&self, config: &mut Config) {
        config.ignore_scripts = resolve_bool_override(
            self.ignore_scripts,
            self.no_ignore_scripts,
            config.ignore_scripts,
        );
        config.ignore_workspace_root_check = resolve_bool_override(
            self.ignore_workspace_root_check,
            self.no_ignore_workspace_root_check,
            config.ignore_workspace_root_check,
        );
        config.optional = resolve_bool_override(self.optional, self.no_optional, config.optional);
        config.ignore_pnpmfile = self.ignore_pnpmfile || config.ignore_pnpmfile;
        config.force = self.force || config.force;
    }

    /// The `--config` selectors parsed into the `name → specifier` pairs to
    /// record, or `None` when `--config` was not passed.
    ///
    /// Callers must run this *before* [`State::init`]: that scaffolds a
    /// `package.json` on disk, so rejecting an invalid selector afterwards
    /// would leave a half-created project behind. A version-less selector
    /// resolves the `latest` tag, matching the default `add` behavior.
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

    /// Execute the subcommand. `config_dependencies` is
    /// [`Self::parse_config_dependencies`]'s output, so it is `Some` exactly
    /// when `--config` was passed.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        config_dependencies: Option<BTreeMap<String, String>>,
    ) -> miette::Result<()> {
        // `--config` routes to the configurational-dependency path
        // instead of the regular `package.json` add: resolve + install
        // into `.pnpm-config`, then record the clean specifiers in
        // `pnpm-workspace.yaml`.
        if let Some(added) = config_dependencies {
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

        let mut state = state;
        let pins = record_package_manager_pins(&mut state, &self.package_names).await?;
        if pins.remaining.is_empty() {
            pins.save(&mut state)?;
            pins.report::<Reporter>();
            return Ok(());
        }
        let package_names = pins.remaining.clone();

        let range_spec_style = self.range_spec_style(state.config);
        let dependency_options =
            self.dependency_options.clone().with_save_peer_setting(state.config.save_peer);

        // The install saves the manifest, so the declarations recorded
        // above reach disk with the dependencies or not at all.
        add_packages::<Reporter, _>(
            state,
            &package_names,
            range_spec_style,
            save_catalog_name,
            self.lockfile_only,
            supported_architectures,
            dependency_options.save_target(),
        )
        .await?;
        pins.report::<Reporter>();
        Ok(())
    }

    pub(crate) async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
        selection: InstallFamilySelection,
    ) -> miette::Result<()> {
        // Which package manager a project uses is that project's own
        // declaration, so it is recorded where the command runs rather
        // than written into each project a filter happens to select.
        // Refusing is the honest answer: the alternative is installing
        // the npm package that shares the name, which is not what naming
        // a package manager asks for anywhere else.
        if let Some(request) =
            self.package_names.iter().find(|request| declared_package_manager(request).is_some())
        {
            return Err(AddError::PackageManagerInSelection { request: request.clone() }.into());
        }
        let supported_architectures =
            self.supported_architectures.apply_to(state.config.supported_architectures.clone());
        let save_catalog_name = self
            .save_catalog_name
            .clone()
            .or_else(|| self.save_catalog.then(|| "default".to_string()))
            .or_else(|| state.config.save_catalog_name.clone());
        let range_spec_style = self.range_spec_style(state.config);
        let dependency_groups = self
            .dependency_options
            .clone()
            .with_save_peer_setting(state.config.save_peer)
            .save_target();
        let InstallFamilySelection {
            workspace_root: _,
            workspace_cycles: _,
            mut projects,
            project_dependencies,
            ordered_dirs,
            selected_dirs,
            install_dirs,
            active_manifest_is_standin,
        } = selection;
        let lockfile_path = state.lockfile_path();
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &mut state;
        let lockfile =
            lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

        Add {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            lockfile,
            lockfile_path: Some(&lockfile_path),
            dependency_groups,
            package_names: &self.package_names,
            range_spec_style,
            save_catalog_name,
            resolved_packages,
            supported_architectures,
            lockfile_only: self.lockfile_only,
        }
        .run_selected::<Reporter>(
            &mut projects,
            &project_dependencies,
            &ordered_dirs,
            selected_dirs.as_ref(),
            install_dirs.as_ref(),
            active_manifest_is_standin,
        )
        .await
        .wrap_err("adding a new package")
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
            return Err(miette::miette!("`pnpm add --config` cannot be combined with --global."));
        }
        if self.lockfile_only {
            return Err(miette::miette!(
                "`pnpm add --lockfile-only` cannot be combined with --global."
            ));
        }
        let supported_architectures =
            self.supported_architectures.apply_to(config.supported_architectures.clone());
        let range_spec_style = self.range_spec_style(config);
        Box::pin(crate::cli_args::global::handle_global_add::<Reporter>(
            config,
            &self.package_names,
            range_spec_style,
            supported_architectures,
            &self.allow_build,
            dir,
        ))
        .await
    }

    /// The style that decides the saved range: `--save-exact` /
    /// `--save-prefix` layered over the `saveExact` and `savePrefix`
    /// settings, mirroring pnpm's `getRangeSpecStyle`.
    fn range_spec_style(&self, config: &Config) -> RangeSpecStyle {
        RangeSpecStyle::from_save_options(
            self.save_exact || config.save_exact,
            self.save_prefix.as_deref().or(config.save_prefix.as_deref()),
        )
    }
}

/// Honor `--allow-build`: reject any package the root project explicitly
/// disallows (`allowBuilds: false`), persist the allowed names to
/// `settings_dir`'s `pnpm-workspace.yaml`, and enable them for this
/// install. `settings_dir` is the workspace root, or the project
/// directory outside a workspace. Mirrors pnpm's `add` handler; shared by
/// the workspace and `--global` add paths.
pub(crate) fn apply_allow_build(
    config: &mut Config,
    allow_build: &[String],
    settings_dir: &Path,
) -> miette::Result<()> {
    if allow_build.is_empty() {
        return Ok(());
    }
    let overlap: Vec<&str> = allow_build
        .iter()
        .filter(|pkg| config.allow_builds.get(pkg.as_str()) == Some(&false))
        .map(String::as_str)
        .collect();
    if !overlap.is_empty() {
        return Err(AllowBuildError::OverridingIgnoredBuiltDependencies {
            dependencies: overlap.join(", "),
        }
        .into());
    }
    set_allow_builds(settings_dir, allow_build.iter().map(|pkg| (pkg.as_str(), true)))
        .into_diagnostic()?;
    for pkg in allow_build {
        config.allow_builds.insert(pkg.clone(), true);
    }
    Ok(())
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum AddError {
    #[display(
        "Running this command will add the dependency to the workspace root, which might not be what you want - if you really meant it, make it explicit by running this command again with the -w flag (or --workspace-root). If you don't want to see this warning anymore, you may set the ignore-workspace-root-check setting to true."
    )]
    #[diagnostic(code(ERR_PNPM_ADDING_TO_ROOT))]
    AddingToRoot,

    #[display(
        "Cannot declare {request} as the package manager of a filtered selection of projects"
    )]
    #[diagnostic(
        code(ERR_PNPM_PACKAGE_MANAGER_IN_SELECTION),
        help(
            "Which package manager a project uses is declared in that project. Run the command in the project itself, without a filter."
        )
    )]
    PackageManagerInSelection {
        #[error(not(source))]
        request: String,
    },
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum AllowBuildError {
    #[display(
        "The following dependencies are ignored by the root project, but are allowed to be built by the current command: {dependencies}"
    )]
    #[diagnostic(
        code(ERR_PNPM_OVERRIDING_IGNORED_BUILT_DEPENDENCIES),
        help(
            "If you are sure you want to allow those dependencies to run installation scripts, remove them from the allowBuilds list (or change their value to true)."
        )
    )]
    OverridingIgnoredBuiltDependencies { dependencies: String },
}

/// Add a single package to `state`'s manifest and install it.
///
/// Shared by `pacquet dlx`, `pacquet runtime`, and the self-updater. dlx
/// points `state` at a cache directory (via a [`Config`] whose `modules_dir`
/// is anchored there) and saves to `dependencies` so the package's bin lands
/// in `<cacheDir>/node_modules/.bin`.
pub(crate) async fn add_package<Reporter, DependencyGroupList>(
    state: State,
    package_name: &str,
    range_spec_style: RangeSpecStyle,
    save_catalog_name: Option<String>,
    lockfile_only: bool,
    supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    dependency_groups: DependencyGroupList,
) -> miette::Result<()>
where
    Reporter: self::Reporter + 'static,
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    let package_names = [package_name.to_string()];
    Box::pin(add_packages::<Reporter, _>(
        state,
        &package_names,
        range_spec_style,
        save_catalog_name,
        lockfile_only,
        supported_architectures,
        Some(dependency_groups),
    ))
    .await
}

/// What [`record_package_manager_pins`] made of a command's requests.
struct RecordedPins {
    /// The requests left to install. Empty when the whole command was
    /// package managers and there is nothing to install.
    remaining: Vec<String>,
    /// The declarations written into the manifest, as they read back.
    recorded: Vec<String>,
}

/// Record every package manager among `package_names` as the one the
/// project uses, into `state`'s in-memory manifest.
///
/// The manifest is not saved here: an `add` that also installs something
/// saves it once the install succeeds, so a failed command leaves the
/// project as it found it. [`RecordedPins::save`] is for the command that
/// installs nothing.
///
/// A runtime is left in the list: unlike a package manager it is
/// installed, and [`tool_install_selector`] turns it into the `runtime:`
/// request that records it under `engines.runtime`.
///
/// pnpm's own pin is deliberately not written here. Changing it makes the
/// next command switch the running CLI, which is `pnpm self-update`'s job
/// to do deliberately rather than an `add`'s to do as a side effect.
async fn record_package_manager_pins(
    state: &mut State,
    package_names: &[String],
) -> miette::Result<RecordedPins> {
    let mut remaining = Vec::new();
    let mut recorded = Vec::new();
    for request in package_names {
        if let Some((pm, version_spec)) = declared_package_manager(request) {
            let reference = resolve_project_pin(state.config, pm, version_spec.as_deref()).await?;
            let reference = reference.as_deref();
            let manifest = state
                .manifest
                .value_mut()
                .as_object_mut()
                .ok_or(EngineError::ManifestIsNotAnObject)?;
            record_package_manager_pin(manifest, pm, reference);
            recorded.push(describe_pin(pm, reference));
        } else {
            let selector = tool_install_selector(request);
            remaining.push(selector.unwrap_or_else(|| request.clone()));
        }
    }
    Ok(RecordedPins { remaining, recorded })
}

impl RecordedPins {
    /// Save the declarations, for a command with nothing to install.
    fn save(&self, state: &mut State) -> miette::Result<()> {
        if self.recorded.is_empty() {
            return Ok(());
        }
        state.manifest.save().map_err(miette::Report::new).wrap_err("save the manifest")
    }

    /// Report what was declared, once it is on disk.
    fn report<Reporter: self::Reporter>(&self) {
        for pin in &self.recorded {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Info,
                message: format!("Recorded {pin} as the project's package manager"),
                prefix: String::new(),
            }));
        }
    }
}

/// Add packages to `state`'s manifest and install them in one operation.
pub(crate) async fn add_packages<Reporter, DependencyGroupList>(
    mut state: State,
    package_names: &[String],
    range_spec_style: RangeSpecStyle,
    save_catalog_name: Option<String>,
    lockfile_only: bool,
    supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    dependency_groups: Option<DependencyGroupList>,
) -> miette::Result<()>
where
    Reporter: self::Reporter + 'static,
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    let lockfile_path = state.lockfile_path();
    let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
        &mut state;
    let lockfile =
        lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

    Add {
        tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
        http_client,
        http_client_arc: std::sync::Arc::clone(http_client),
        config,
        manifest,
        lockfile,
        lockfile_path: Some(&lockfile_path),
        dependency_groups,
        package_names,
        range_spec_style,
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
