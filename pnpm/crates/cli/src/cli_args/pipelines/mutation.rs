use super::{
    AddArgs, Arc, BTreeMap, Config, Context, DedicatedProjectRuns, DeployArgs,
    EcosystemPackageSpecifier, InstallFamily, InstallFamilyPlan, Path, PathBuf, RemoveArgs,
    Reporter, State, ThrottledClient, UpdateArgs, UpdateChangesetContext, anchor_active_project,
    config_deps, dedicated_project_name, ecosystem_add, ecosystem_install, init_shared_state,
    select_install_family, select_install_family_plan,
};
use crate::cli_args::recursive::UnmatchedFilters;

pub(crate) struct AddPipeline {
    pub(crate) args: AddArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
    /// [`AddArgs::parse_config_dependencies`]'s output, parsed by the dispatch
    /// before this pipeline scaffolds a manifest. `Some` exactly when
    /// `--config` was passed.
    pub(crate) config_dependencies: Option<BTreeMap<String, String>>,
    /// The selectors routed away from the npm add path, which receives
    /// the rest through [`AddArgs::package_names`].
    pub(crate) ecosystem_packages: Vec<EcosystemPackageSpecifier>,
}

impl AddPipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        config_deps::prepare::<Reporter>(self.cfg, &self.config_root, false).await?;
        if !self.ecosystem_packages.is_empty() {
            return EcosystemAdd {
                args: self.args,
                cfg: self.cfg,
                prefix: self.prefix,
                manifest_path: self.manifest_path,
                recursive_sort: self.recursive_sort,
                ecosystem_packages: self.ecosystem_packages,
            }
            .run::<Reporter>()
            .await;
        }
        // `--config` targets the workspace's configuration dependencies, not
        // any project's manifest, so it bypasses project selection entirely.
        let plan = if self.config_dependencies.is_some() {
            InstallFamilyPlan::Single
        } else {
            select_install_family_plan::<Reporter>(
                self.cfg,
                &self.prefix,
                &self.manifest_path,
                self.recursive_sort,
                true,
                false,
            )?
        };
        self.run_plan::<Reporter>(plan).await
    }

    pub(super) async fn run_plan<Reporter: self::Reporter + 'static>(
        self,
        plan: InstallFamilyPlan,
    ) -> miette::Result<()> {
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                // Dedicated per-project lockfiles: add the packages to each
                // selected project independently.
                let workspace_packages = self.args.workspace_link_targets(self.cfg)?;
                DedicatedProjectRuns {
                    config: self.cfg,
                    projects,
                    require_lockfile: false,
                    http_client: None,
                }
                .run(|state| {
                    Box::pin(
                        self.args
                            .clone()
                            .run_with_link_targets::<Reporter>(
                                state,
                                None,
                                workspace_packages.as_ref(),
                            ),
                    )
                })
                .await
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let cfg: &'static Config = self.cfg;
                let state =
                    State::init(self.manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(self.args.run_selected::<Reporter>(state, *selection)).await
            }
            InstallFamilyPlan::Single => self.run_single::<Reporter>().await,
        }
    }

    async fn run_single<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        // Dedicated per-project lockfiles: `add` mutates only the
        // active project, whose outputs anchor at the project dir.
        // `--config` targets the workspace's configuration
        // dependencies, which stay workspace-anchored.
        if self.config_dependencies.is_none()
            && !self.cfg.shares_one_lockfile()
            && self.cfg.workspace_dir.is_some()
        {
            anchor_active_project(self.cfg, &self.manifest_path);
        }
        let cfg: &'static Config = self.cfg;
        let state = State::init(self.manifest_path, cfg, false).wrap_err("initialize the state")?;
        Box::pin(self.args.run::<Reporter>(state, self.config_dependencies)).await
    }
}

/// A `pnpm add` that carries at least one Cargo or Python package, named
/// by a `crate:` or `pypi:` selector or by a Package URL.
struct EcosystemAdd {
    args: AddArgs,
    cfg: &'static mut Config,
    prefix: PathBuf,
    manifest_path: PathBuf,
    recursive_sort: bool,
    ecosystem_packages: Vec<EcosystemPackageSpecifier>,
}

/// The npm half of an add that also carries ecosystem packages, enrolled in
/// the same transaction as they are.
struct NodeAdd {
    args: AddArgs,
    cfg: &'static Config,
    manifest_path: PathBuf,
    http_client: Arc<ThrottledClient>,
}

impl EcosystemAdd {
    async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let has_node_packages = !self.args.package_names.is_empty();
        let EcosystemAddSetup { cfg, http_client, family } = prepare_ecosystem_add::<Reporter>(
            self.cfg,
            (&self.prefix, &self.manifest_path),
            self.recursive_sort,
            has_node_packages,
        )?;
        let ecosystem = ecosystem_add::plan::<Reporter>(
            add_install_context(cfg, &http_client, &self.args),
            self.prefix,
            self.ecosystem_packages,
            &self.args,
            has_node_packages,
            family.scope.as_ref(),
        )
        .await?;
        if let Some(unmatched) = unmatched_after_ecosystems(family.unmatched, &ecosystem) {
            return Err(unmatched);
        }
        if !has_node_packages {
            return ecosystem.plan.run().await;
        }
        run_mixed_add::<Reporter>(
            ecosystem.plan,
            NodeAdd { args: self.args, cfg, manifest_path: self.manifest_path, http_client },
        )
        .await
    }
}

struct EcosystemAddSetup {
    cfg: &'static Config,
    http_client: Arc<ThrottledClient>,
    family: InstallFamily,
}

fn prepare_ecosystem_add<Reporter: self::Reporter>(
    cfg: &'static mut Config,
    (prefix, manifest_path): (&Path, &Path),
    recursive_sort: bool,
    has_node_packages: bool,
) -> miette::Result<EcosystemAddSetup> {
    if !cfg.shares_one_lockfile() && cfg.workspace_dir.is_some() && has_node_packages {
        anchor_dedicated_add_target(cfg, manifest_path);
    }
    let http_client = State::new_http_client(cfg).wrap_err("initialize the add network")?;
    let cfg: &'static Config = cfg;
    // The npm projects the selection resolved to, which is what the
    // ecosystems in the selected directories are added to.
    let family =
        select_install_family::<Reporter>(cfg, prefix, manifest_path, recursive_sort, true, false)?;
    Ok(EcosystemAddSetup { cfg, http_client, family })
}

fn add_install_context(
    cfg: &'static Config,
    http_client: &Arc<ThrottledClient>,
    args: &AddArgs,
) -> ecosystem_install::InstallContext {
    ecosystem_install::InstallContext {
        config: cfg,
        http_client: Arc::clone(http_client),
        lockfile_only: args.install.lockfile_only,
        frozen_lockfile: false,
    }
}

/// The empty-selection failure the whole command earns, if any. A selector
/// that named no npm project may have named a Python one.
fn unmatched_after_ecosystems(
    unmatched: Option<UnmatchedFilters>,
    ecosystem: &ecosystem_install::EcosystemPlan,
) -> Option<miette::Report> {
    unmatched
        .filter(|_| ecosystem.python.selected == 0)
        .map(|unmatched| unmatched.counting(ecosystem.python.discovered).report())
}

/// Anchor the dedicated-lockfile project the npm half of the add writes to,
/// so its outputs land beside its own manifest.
fn anchor_dedicated_add_target(cfg: &mut Config, manifest_path: &Path) {
    let manifest_dir = manifest_path
        .parent()
        .expect("manifest path always has a parent dir")
        .to_path_buf();
    let name = dedicated_project_name(cfg, &manifest_dir);
    cfg.anchor_dedicated_project(&manifest_dir, name.as_deref());
}

async fn run_mixed_add<Reporter: self::Reporter + 'static>(
    plan: pnpm_install_coordinator::InstallPlan<'static>,
    node: NodeAdd,
) -> miette::Result<()> {
    let NodeAdd {
        args,
        cfg,
        manifest_path,
        http_client,
    } = node;
    let metadata = node_add_metadata_paths(cfg, &manifest_path);
    let node_install = async move {
        let state = init_shared_state(manifest_path, cfg, false, None, http_client)?;
        Box::pin(args.run::<Reporter>(state, None)).await
    };
    plan.with_task(pnpm_install_coordinator::InstallTask::in_place(metadata, node_install))
        .run()
        .await
}

pub(super) fn node_add_metadata_paths(config: &Config, manifest_path: &Path) -> Vec<PathBuf> {
    let project_dir = manifest_path.parent().expect("manifest path always has a parent dir");
    let mut paths = vec![
        manifest_path.to_path_buf(),
        config.lockfile_dir_for(project_dir).join(config.wanted_lockfile_name()),
        // The current lockfile remains project-local even with a global virtual store.
        config.virtual_store_dir.join(pnpm_lockfile::Lockfile::CURRENT_FILE_NAME),
        config.modules_dir.join(pnpm_modules_yaml::MODULES_FILENAME),
    ];
    if let Some(workspace_dir) = config.workspace_dir.as_deref() {
        paths.push(workspace_dir.join("pnpm-workspace.yaml"));
    }
    paths
}

#[cfg(test)]
#[path = "add_metadata_paths_tests.rs"]
mod add_metadata_paths_tests;

pub(crate) struct UpdatePipeline {
    pub(crate) args: UpdateArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
}

impl UpdatePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        config_deps::prepare::<Reporter>(self.cfg, &self.config_root, false).await?;
        let plan = select_install_family_plan::<Reporter>(
            self.cfg,
            &self.prefix,
            &self.manifest_path,
            self.recursive_sort,
            false,
            false,
        )?;
        // An empty selection has nothing to update, and — like the shared
        // path — must not generate a changeset.
        match &plan {
            InstallFamilyPlan::PerProject(projects) if projects.is_empty() => {
                return Ok(());
            }
            InstallFamilyPlan::Shared(selection) if selection.selected_dirs.is_empty() => {
                return Ok(());
            }
            _ => {}
        }
        // Dedicated per-project lockfiles: the non-recursive command
        // mutates only the active project, whose outputs anchor at the
        // project dir.
        if matches!(plan, InstallFamilyPlan::Single)
            && !self.cfg.shares_one_lockfile()
            && self.cfg.workspace_dir.is_some()
        {
            anchor_active_project(self.cfg, &self.manifest_path);
        }
        let generate_changeset = if self.args.save.changeset {
            true
        } else if self.args.save.no_changeset {
            false
        } else {
            self.cfg.update_config.changeset.unwrap_or(false)
        };
        let changeset_context = generate_changeset
            .then(|| UpdateChangesetContext::capture(self.cfg, &self.manifest_path))
            .transpose()?;
        self.run_plan::<Reporter>(plan).await?;
        if let Some(changeset_context) = changeset_context {
            changeset_context.generate::<Reporter>()?;
        }
        Ok(())
    }
    pub(super) async fn run_plan<Reporter: self::Reporter + 'static>(
        self,
        plan: InstallFamilyPlan,
    ) -> miette::Result<()> {
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                DedicatedProjectRuns {
                    config: self.cfg,
                    projects,
                    require_lockfile: false,
                    http_client: None,
                }
                .run(|state| Box::pin(self.args.clone().run::<Reporter>(state)))
                .await?;
            }
            InstallFamilyPlan::Shared(selection) => {
                let cfg: &'static Config = self.cfg;
                let state =
                    State::init(self.manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(self.args.run_selected::<Reporter>(state, *selection)).await?;
            }
            InstallFamilyPlan::Single => {
                let cfg: &'static Config = self.cfg;
                let state =
                    State::init(self.manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(self.args.run::<Reporter>(state)).await?;
            }
        }
        Ok(())
    }
}

pub(crate) struct RemovePipeline {
    pub(crate) args: RemoveArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
}

impl RemovePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        config_deps::prepare::<Reporter>(self.cfg, &self.config_root, false).await?;
        let plan = select_install_family_plan::<Reporter>(
            self.cfg,
            &self.prefix,
            &self.manifest_path,
            self.recursive_sort,
            false,
            false,
        )?;
        self.run_plan::<Reporter>(plan).await
    }

    async fn run_plan<Reporter: self::Reporter + 'static>(
        self,
        plan: InstallFamilyPlan,
    ) -> miette::Result<()> {
        let Self { args, cfg, manifest_path, .. } = self;
        match plan {
            InstallFamilyPlan::PerProject(projects) => {
                // Dedicated per-project lockfiles: remove the packages from
                // each selected project independently.
                DedicatedProjectRuns {
                    config: cfg,
                    projects,
                    require_lockfile: false,
                    http_client: None,
                }
                .run(|state| Box::pin(args.clone().run::<Reporter>(state)))
                .await
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run_selected::<Reporter>(state, *selection)).await
            }
            InstallFamilyPlan::Single => {
                // Dedicated per-project lockfiles: the non-recursive command
                // mutates only the active project, whose outputs anchor at the
                // project dir.
                if !cfg.shares_one_lockfile() && cfg.workspace_dir.is_some() {
                    anchor_active_project(cfg, &manifest_path);
                }
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run::<Reporter>(state)).await
            }
        }
    }
}

pub(crate) struct DeployPipeline {
    pub(crate) args: DeployArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
}

impl DeployPipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir_ref: &Path,
    ) -> miette::Result<()> {
        let DeployPipeline { args, cfg, config_root } = self;
        config_deps::prepare::<Reporter>(cfg, &config_root, false).await?;
        let cfg: &'static Config = cfg;
        Box::pin(args.run::<Reporter>(cfg, dir_ref)).await
    }
}
