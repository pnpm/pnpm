use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use std::{future::Future, pin::Pin, sync::Arc};

mod metadata_file;
mod mutation;
mod workspace_inventory;

pub(crate) use mutation::MetadataMutation;
pub(crate) use workspace_inventory::EcosystemWorkspaceInventory;

type Installer<'a> = Pin<Box<dyn Future<Output = miette::Result<()>> + Send + 'a>>;

/// Report whether a non-Node.js ecosystem participates in this install.
pub(crate) fn is_enabled(config: &Config) -> bool {
    config.cargo.enabled
}

pub(crate) struct InstallContext {
    pub(crate) config: &'static Config,
    pub(crate) http_client: Arc<ThrottledClient>,
    pub(crate) lockfile_only: bool,
    pub(crate) frozen_lockfile: bool,
}

/// Coordinates dependency installation across every configured ecosystem.
pub(crate) struct EcosystemInstallCoordinator<'a> {
    installers: Vec<Installer<'a>>,
}

impl<'a> EcosystemInstallCoordinator<'a> {
    pub(crate) fn new<Install>(install: Install) -> Self
    where
        Install: Future<Output = miette::Result<()>> + Send + 'a,
    {
        Self { installers: vec![Box::pin(install)] }
    }

    pub(crate) fn with_install<Install>(mut self, install: Install) -> Self
    where
        Install: Future<Output = miette::Result<()>> + Send + 'a,
    {
        self.installers.push(Box::pin(install));
        self
    }

    pub(crate) async fn run(self) -> miette::Result<()> {
        futures_util::future::try_join_all(self.installers).await?;
        Ok(())
    }

    pub(crate) async fn run_to_settlement(self) -> miette::Result<()> {
        futures_util::future::join_all(self.installers).await.into_iter().collect()
    }
}

#[cfg(test)]
mod tests;
