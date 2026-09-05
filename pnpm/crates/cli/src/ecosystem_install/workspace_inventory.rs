use miette::{IntoDiagnostic, Result, WrapErr};
use std::path::PathBuf;
use tokio::sync::OnceCell;

const MANIFEST_BASENAMES: &[&str] = &["Cargo.toml"];
const IGNORED_DIRECTORY_BASENAMES: &[&str] = &[".git", ".pnpm", "node_modules", "target"];

/// Lazily populated manifest inventory shared by enabled ecosystem installers.
pub(crate) struct EcosystemWorkspaceInventory {
    workspace_root: PathBuf,
    contents: OnceCell<pnpm_workspace::WorkspaceInventory>,
}

impl EcosystemWorkspaceInventory {
    pub(crate) fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root, contents: OnceCell::new() }
    }

    pub(crate) async fn manifests(&self, basename: &str) -> Result<&[PathBuf]> {
        let inventory = self
            .contents
            .get_or_try_init(|| {
                let workspace_root = self.workspace_root.clone();
                async move {
                    tokio::task::spawn_blocking(move || {
                        pnpm_workspace::find_workspace_inventory(
                            &workspace_root,
                            MANIFEST_BASENAMES,
                            IGNORED_DIRECTORY_BASENAMES,
                        )
                    })
                    .await
                    .into_diagnostic()
                    .wrap_err("join ecosystem workspace discovery task")?
                    .into_diagnostic()
                }
            })
            .await?;
        Ok(inventory.manifests(basename))
    }
}

#[cfg(test)]
mod tests;
