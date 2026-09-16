pub(crate) use workspace_inventory::{EcosystemManifest, EcosystemWorkspaceInventory};

pub(crate) mod python;
mod workspace_inventory;

use crate::{
    cargo_deps,
    cli_args::{install::InstallDependencyOptions, pipelines::WorkspaceScope},
};
use pnpm_config::Config;
use pnpm_install_coordinator::InstallPlan;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::DependencyGroup;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Report whether a non-Node.js ecosystem participates in this install.
pub(crate) fn is_enabled(config: &Config) -> bool {
    config.cargo.enabled || config.python.enabled
}

#[derive(Clone)]
pub(crate) struct InstallContext {
    pub(crate) config: &'static Config,
    pub(crate) http_client: Arc<ThrottledClient>,
    pub(crate) lockfile_only: bool,
    pub(crate) frozen_lockfile: bool,
}

/// An install plan for the non-npm ecosystems, and the Python projects it
/// was resolved against.
pub(crate) struct EcosystemPlan {
    pub(crate) plan: InstallPlan<'static>,
    pub(crate) python: PythonProjects,
}

/// How many Python projects the workspace holds, and how many of them the
/// selection asked for. Together they decide an empty `--filter`
/// selection: selectors that matched no npm project may still have named a
/// Python one, and a workspace whose projects are all Python is not one
/// that holds no project at all.
#[derive(Default)]
pub(crate) struct PythonProjects {
    pub(crate) discovered: usize,
    pub(crate) selected: usize,
}

pub(crate) async fn plan<Reporter: pnpm_reporter::Reporter + 'static>(
    context: InstallContext,
    root: PathBuf,
    prefix: &Path,
    dependencies: &InstallDependencyOptions,
    scope: Option<&WorkspaceScope>,
) -> miette::Result<EcosystemPlan> {
    let inventory = EcosystemWorkspaceInventory::new(root.clone(), context.config);
    let config = context.config;
    let mut plan = InstallPlan::new(config.workspace_dir.clone().unwrap_or(root));
    let mut python = PythonProjects::default();
    if config.cargo.enabled {
        plan = plan.with_task(cargo_deps::plan::<Reporter>(context.clone(), &inventory).await?);
    }
    if config.python.enabled {
        let groups = dependencies.dependency_groups(config.optional).collect::<Vec<_>>();
        let discovery = python::discover(config, &inventory).await?;
        let selected = python::selected_projects(config, prefix, &discovery, scope)?;
        python = PythonProjects {
            discovered: discovery.project_roots().count(),
            selected: selected.len(),
        };
        plan = plan.with_task(pnpm_python_installer::plan::<Reporter>(
            context.into(),
            discovery,
            pnpm_python_installer::DependencySelection {
                production: groups.contains(&DependencyGroup::Prod),
                development: groups.contains(&DependencyGroup::Dev),
            },
            selected,
        ));
    }
    Ok(EcosystemPlan { plan, python })
}

impl From<InstallContext> for pnpm_python_installer::InstallOptions {
    fn from(context: InstallContext) -> Self {
        Self {
            config: context.config,
            http_client: context.http_client,
            lockfile_only: context.lockfile_only,
            frozen_lockfile: context.frozen_lockfile,
        }
    }
}
