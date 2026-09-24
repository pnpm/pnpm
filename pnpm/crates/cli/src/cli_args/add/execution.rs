use super::{
    Add, AddArgs, AddError, AddRequest, BTreeMap, Config, Context, DependencyGroup, EngineError,
    InstallFamilySelection, LogEvent, LogLevel, Path, PathBuf, PnpmLog, RangeSpecStyle, Reporter,
    State, WorkspacePackages, build_workspace_packages_map, config_deps, declared_package_manager,
    describe_pin, record_package_manager_pin, resolve_project_pin, tool_install_selector,
    workspace_link_root, workspace_selectors,
};
use crate::state::command_lockfile;

/// The dependency groups one add works with, which are two different
/// sets: `save_target` names the manifest group the added packages are
/// written to, `included` the groups the install that follows resolves
/// and materializes (`--prod` / `--dev`). `None` defaults each, as
/// documented on the [`pnpm_package_manager::AddResources`] fields they
/// reach.
pub(crate) struct AddGroups<DependencyGroupList> {
    pub(crate) save_target: Option<DependencyGroupList>,
    pub(crate) included: Option<Vec<DependencyGroup>>,
    pub(crate) save_types: bool,
}

/// Add a single package to `state`'s manifest and install it.
///
/// Shared by `pacquet dlx`, `pacquet runtime`, and the self-updater. dlx
/// points `state` at a cache directory (via a [`Config`] whose `modules_dir`
/// is anchored there) and saves to `dependencies` so the package's bin lands
/// in `<cacheDir>/node_modules/.bin`.
///
/// `policy_excludes_dir` overrides where the install persists policy
/// excludes (`minimumReleaseAgeExclude`, ...); `None` persists to the
/// install's own lockfile directory. The self-updater passes the invoking
/// project's workspace: its engine install runs in a throwaway directory
/// whose manifest is deleted right after the install, so without this an
/// approval at the minimum-release-age prompt would persist nowhere
/// (pnpm/pnpm#15396).
#[expect(
    clippy::too_many_arguments,
    reason = "one flat parameter list for every axis an add needs; the engine-install caller alone sets policy_excludes_dir"
)]
pub(crate) async fn add_package<Reporter, DependencyGroupList>(
    state: State,
    package_name: &str,
    range_spec_style: RangeSpecStyle,
    save_catalog_name: Option<String>,
    lockfile_only: bool,
    supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    dependency_groups: DependencyGroupList,
    policy_excludes_dir: Option<&Path>,
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
        AddGroups { save_target: Some(dependency_groups), included: None, save_types: false },
        policy_excludes_dir,
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
    requests: &[AddRequest],
) -> miette::Result<RecordedPins> {
    let mut remaining = Vec::new();
    let mut recorded = Vec::new();
    for request in requests {
        let selector = request.selector();
        if !request.may_name_a_tool() {
            remaining.push(selector.to_string());
            continue;
        }
        if let Some((pm, version_spec)) = declared_package_manager(selector) {
            let reference = resolve_project_pin(state.config, pm, version_spec.as_deref()).await?;
            let reference = reference.as_deref();
            let manifest = state.manifest
                .value_mut()
                .as_object_mut()
                .ok_or(EngineError::ManifestIsNotAnObject)?;
            record_package_manager_pin(manifest, pm, reference);
            recorded.push(describe_pin(pm, reference));
        } else {
            let tool = tool_install_selector(selector);
            remaining.push(tool.unwrap_or_else(|| selector.to_string()));
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
        state.manifest
            .save()
            .map_err(miette::Report::new)
            .wrap_err("save the manifest")
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
#[expect(
    clippy::too_many_arguments,
    reason = "one flat parameter list for every axis an add needs; the engine-install caller alone sets policy_excludes_dir"
)]
pub(crate) async fn add_packages<Reporter, DependencyGroupList>(
    mut state: State,
    package_names: &[String],
    range_spec_style: RangeSpecStyle,
    save_catalog_name: Option<String>,
    lockfile_only: bool,
    supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    groups: AddGroups<DependencyGroupList>,
    policy_excludes_dir: Option<&Path>,
) -> miette::Result<()>
where
    Reporter: self::Reporter + 'static,
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    let lockfile_path = state.lockfile_path();
    let State {
        tarball_mem_cache,
        http_client,
        config,
        manifest,
        lockfile,
        resolved_packages,
    } = &mut state;
    let lockfile = command_lockfile(lockfile, &lockfile_path)?;

    Add {
        manifest,
        options: pnpm_package_manager::AddOptions {
            http_client,
            config,
            lockfile,
            package_names,
            range_spec_style,
            resolved_packages,
            lockfile_only,
            save_types: groups.save_types,
        },
        resources: pnpm_package_manager::AddResources {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client_arc: std::sync::Arc::clone(http_client),
            dependency_groups: groups.save_target,
            included_groups: groups.included,
            save_catalog_name,
            supported_architectures,
            policy_excludes_dir: policy_excludes_dir.map(Path::to_path_buf),
        },
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
    let root_dir = state.config.workspace_dir
        .clone()
        .unwrap_or_else(|| {
            state.manifest
                .path()
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
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
        let included_groups = self.included_groups(state.config);
        let save_types = state.config.save_types;

        // The install saves the manifest, so the declarations recorded
        // above reach disk with the dependencies or not at all.
        add_packages::<Reporter, _>(
            state,
            &package_names,
            range_spec_style,
            save_catalog_name,
            self.install.lockfile_only,
            supported_architectures,
            AddGroups {
                save_target: dependency_options.save_target(),
                included: Some(included_groups),
                save_types,
            },
            // A plain `add` installs into the project's own workspace, so
            // the default — the install's lockfile directory — is the
            // `pnpm-workspace.yaml` the excludes belong in.
            None,
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
        let dependency_groups = self.dependency_options
            .clone()
            .with_save_peer_setting(state.config.save_peer)
            .save_target();
        let included_groups = self.included_groups(state.config);
        let lockfile_path = state.lockfile_path();
        let lockfile = command_lockfile(&state.lockfile, &lockfile_path)?;

        Add {
            manifest: &mut state.manifest,
            options: pnpm_package_manager::AddOptions {
                http_client: &state.http_client,
                config: state.config,
                lockfile,
                package_names: &package_names,
                range_spec_style: self.range_spec_style(state.config),
                resolved_packages: &state.resolved_packages,
                lockfile_only: self.install.lockfile_only,
                save_types: state.config.save_types,
            },
            resources: pnpm_package_manager::AddResources {
                tarball_mem_cache: std::sync::Arc::clone(&state.tarball_mem_cache),
                http_client_arc: std::sync::Arc::clone(&state.http_client),
                dependency_groups,
                included_groups: Some(included_groups),
                save_catalog_name,
                supported_architectures,
                // A filtered `add` still installs into the projects' own
                // workspace: the install's lockfile directory is the right
                // place for policy excludes.
                policy_excludes_dir: None,
            },
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
        if let Some(request) = self.package_names
            .iter()
            .find(|request| {
                request.may_name_a_tool() && declared_package_manager(request.selector()).is_some()
            })
        {
            let request = request.selector().to_string();
            return Err(AddError::PackageManagerInSelection { request }.into());
        }
        let selectors: Vec<String> = self.package_names
            .iter()
            .map(|request| request.selector().to_string())
            .collect();
        let package_names =
            match workspace_link_root(self.target.workspace, config.workspace_dir.as_deref())? {
                Some(_) => workspace_selectors(
                    &selectors,
                    &build_workspace_packages_map(Some(&selection.projects)).unwrap_or_default(),
                )?,
                None => selectors,
            };
        Ok(package_names)
    }

    fn effective_save_catalog_name(&self, config: &Config) -> Option<String> {
        self.save.catalog_name
            .clone()
            .or_else(|| self.save.catalog.then(|| "default".to_string()))
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
        if self.target.config {
            return Err(miette::miette!("`pnpm add --config` cannot be combined with --global."));
        }
        if self.install.lockfile_only {
            return Err(miette::miette!(
                "`pnpm add --lockfile-only` cannot be combined with --global."
            ));
        }
        workspace_link_root(self.target.workspace, None)?;
        let supported_architectures =
            self.supported_architectures.apply_to(config.supported_architectures.clone());
        let range_spec_style = self.range_spec_style(config);
        Box::pin(crate::cli_args::global::handle_global_add::<Reporter>(
            config,
            &self.package_names,
            range_spec_style,
            supported_architectures,
            &self.install.allow_build,
            dir,
        ))
        .await
    }
}
