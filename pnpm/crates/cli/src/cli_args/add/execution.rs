use super::{
    Add, AddArgs, AddError, BTreeMap, Config, Context, DependencyGroup, EngineError,
    InstallFamilySelection, LogEvent, LogLevel, Path, PathBuf, PnpmLog, RangeSpecStyle, Reporter,
    State, WorkspacePackages, build_workspace_packages_map, config_deps, declared_package_manager,
    describe_pin, record_package_manager_pin, resolve_project_pin, tool_install_selector,
    workspace_link_root, workspace_selectors,
};

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
    pub(super) fn save(&self, state: &mut State) -> miette::Result<()> {
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

/// Config dependencies are declared at the workspace root, or the single project root.
async fn add_workspace_config_dependencies<Reporter: self::Reporter>(
    state: &State,
    added: &BTreeMap<String, String>,
) -> miette::Result<()> {
    let root_dir = state.config.workspace_dir.clone().unwrap_or_else(|| {
        state.manifest.path().parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    });
    config_deps::add_config_dependencies::<Reporter>(state.config, &root_dir, added).await
}

impl AddArgs {
    /// Execute the subcommand. `config_dependencies` is
    /// [`Self::parse_config_dependencies`]'s output, so it is `Some` exactly
    /// when `--config` was passed.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        config_dependencies: Option<BTreeMap<String, String>>,
    ) -> miette::Result<()> {
        let workspace_packages = self.workspace_link_targets(state.config)?;
        self.run_with_link_targets::<Reporter>(
            state,
            config_dependencies,
            workspace_packages.as_ref(),
        )
        .await
    }

    /// [`Self::run`] with the `--workspace` link targets already indexed
    /// (see [`Self::workspace_link_targets`]), so a plan that runs the add
    /// once per project walks the workspace once.
    pub(crate) async fn run_with_link_targets<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        config_dependencies: Option<BTreeMap<String, String>>,
        workspace_packages: Option<&WorkspacePackages>,
    ) -> miette::Result<()> {
        // `--config` routes to the configurational-dependency path
        // instead of the regular `package.json` add: resolve + install
        // into `.pnpm-config`, then record the clean specifiers in
        // `pnpm-workspace.yaml`.
        if let Some(added) = config_dependencies {
            return add_workspace_config_dependencies::<Reporter>(&state, &added).await;
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
        let save_catalog_name = self.effective_save_catalog_name(state.config);

        let mut state = state;
        let pins = record_package_manager_pins(&mut state, &self.package_names).await?;
        if pins.remaining.is_empty() {
            pins.save(&mut state)?;
            pins.report::<Reporter>();
            return Ok(());
        }
        let package_names = match workspace_packages {
            Some(workspace_packages) => workspace_selectors(&pins.remaining, workspace_packages)?,
            None => pins.remaining.clone(),
        };

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
        mut selection: InstallFamilySelection,
    ) -> miette::Result<()> {
        let package_names = self.selected_package_names(state.config, &selection)?;
        let supported_architectures =
            self.supported_architectures.apply_to(state.config.supported_architectures.clone());
        let save_catalog_name = self.effective_save_catalog_name(state.config);
        let dependency_groups = self
            .dependency_options
            .clone()
            .with_save_peer_setting(state.config.save_peer)
            .save_target();
        let lockfile_path = state.lockfile_path();
        let lockfile = state
            .lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;

        Add {
            tarball_mem_cache: std::sync::Arc::clone(&state.tarball_mem_cache),
            http_client: &state.http_client,
            http_client_arc: std::sync::Arc::clone(&state.http_client),
            config: state.config,
            manifest: &mut state.manifest,
            lockfile,
            lockfile_path: Some(&lockfile_path),
            dependency_groups,
            package_names: &package_names,
            range_spec_style: self.range_spec_style(state.config),
            save_catalog_name,
            resolved_packages: &state.resolved_packages,
            supported_architectures,
            lockfile_only: self.lockfile_only,
        }
        .run_selected::<Reporter>(selection.selected_projects())
        .await
        .wrap_err("adding a new package")
    }

    /// A package-manager pin belongs to the invoking project, not a filtered selection.
    fn selected_package_names(
        &self,
        config: &Config,
        selection: &InstallFamilySelection,
    ) -> miette::Result<Vec<String>> {
        if let Some(request) =
            self.package_names.iter().find(|request| declared_package_manager(request).is_some())
        {
            return Err(AddError::PackageManagerInSelection { request: request.clone() }.into());
        }
        let package_names =
            match workspace_link_root(self.workspace, config.workspace_dir.as_deref())? {
                Some(_) => workspace_selectors(
                    &self.package_names,
                    &build_workspace_packages_map(Some(&selection.projects)).unwrap_or_default(),
                )?,
                None => self.package_names.clone(),
            };
        Ok(package_names)
    }

    fn effective_save_catalog_name(&self, config: &Config) -> Option<String> {
        self.save_catalog_name
            .clone()
            .or_else(|| self.save_catalog.then(|| "default".to_string()))
            .or_else(|| config.save_catalog_name.clone())
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
        workspace_link_root(self.workspace, None)?;
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
}
