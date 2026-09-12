pub(crate) use workspace_inventory::{EcosystemManifest, EcosystemWorkspaceInventory};

mod workspace_inventory;

use crate::{cargo_deps, cli_args::install::InstallDependencyOptions};
use pnpm_config::Config;
use pnpm_install_coordinator::InstallPlan;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::DependencyGroup;
use std::{path::PathBuf, sync::Arc};

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

pub(crate) async fn plan<Reporter: pnpm_reporter::Reporter + 'static>(
    context: InstallContext,
    root: PathBuf,
    dependencies: &InstallDependencyOptions,
) -> miette::Result<InstallPlan<'static>> {
    let inventory = EcosystemWorkspaceInventory::new(root.clone(), context.config);
    let config = context.config;
    let mut plan = InstallPlan::new(config.workspace_dir.clone().unwrap_or(root));
    if config.cargo.enabled {
        plan = plan.with_task(cargo_deps::plan::<Reporter>(context.clone(), &inventory).await?);
    }
    if config.python.enabled {
        let groups = dependencies.dependency_groups(config.optional).collect::<Vec<_>>();
        plan = plan.with_task(pnpm_python_installer::plan::<Reporter>(
            context.into(),
            inventory.manifests(EcosystemManifest::Python).await?.to_vec(),
            pnpm_python_installer::DependencySelection {
                production: groups.contains(&DependencyGroup::Prod),
                development: groups.contains(&DependencyGroup::Dev),
            },
        ));
    }
    Ok(plan)
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
