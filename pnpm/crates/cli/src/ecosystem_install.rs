use crate::cargo_deps;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use std::{future::Future, path::Path, pin::Pin, sync::Arc};

type Installer<'a> = Pin<Box<dyn Future<Output = miette::Result<()>> + Send + 'a>>;

/// Whether any non-Node.js ecosystem participates in this install.
///
/// Keep this check at the orchestration boundary as new ecosystems are added,
/// so an empty Node.js selection can still skip all install initialization.
pub(crate) fn is_enabled(config: &Config) -> bool {
    config.cargo.enabled
}

/// Coordinates dependency installation across every configured ecosystem.
pub(crate) struct EcosystemInstallCoordinator<'a> {
    pub(crate) config: &'a Config,
    pub(crate) root_dir: &'a Path,
    pub(crate) http_client: Arc<ThrottledClient>,
    pub(crate) lockfile_only: bool,
    pub(crate) frozen_lockfile: bool,
}

impl<'a> EcosystemInstallCoordinator<'a> {
    /// Run every configured ecosystem concurrently.
    ///
    /// Each new language contributes one boxed installer here and receives the
    /// same install-wide HTTP client, preserving the configured network budget.
    pub(crate) async fn run<Reporter, NodeInstall>(
        self,
        node_install: NodeInstall,
    ) -> miette::Result<()>
    where
        Reporter: pnpm_reporter::Reporter + 'static,
        NodeInstall: Future<Output = miette::Result<()>> + Send + 'a,
    {
        let cargo_install = cargo_deps::install::<Reporter>(
            self.config,
            self.root_dir,
            self.lockfile_only,
            self.frozen_lockfile,
            Arc::clone(&self.http_client),
        );
        run_installers(vec![Box::pin(node_install), Box::pin(cargo_install)]).await
    }
}

async fn run_installers(installers: Vec<Installer<'_>>) -> miette::Result<()> {
    futures_util::future::try_join_all(installers).await?;
    Ok(())
}

#[cfg(test)]
mod tests;
