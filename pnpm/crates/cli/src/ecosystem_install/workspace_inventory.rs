use miette::{IntoDiagnostic, Result, WrapErr};
use std::path::PathBuf;
use tokio::sync::OnceCell;

const IGNORED_DIRECTORY_BASENAMES: &[&str] = &[".git", ".pnpm", "node_modules", "target"];

#[derive(Clone, Copy)]
pub(crate) enum EcosystemManifest {
    Cargo,
}

impl EcosystemManifest {
    const ALL: &[Self] = &[Self::Cargo];

    const fn basename(self) -> &'static str {
        match self {
            Self::Cargo => "Cargo.toml",
        }
    }
}

/// Manifest paths available to every ecosystem participating in an install.
pub(crate) struct EcosystemWorkspaceInventory {
    workspace_root: PathBuf,
    contents: OnceCell<pnpm_workspace::WorkspaceInventory>,
}

impl EcosystemWorkspaceInventory {
    pub(crate) fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root, contents: OnceCell::new() }
    }

    pub(crate) async fn manifests(&self, manifest: EcosystemManifest) -> Result<&[PathBuf]> {
        let inventory = self
            .contents
            .get_or_try_init(|| {
                let workspace_root = self.workspace_root.clone();
                async move {
                    tokio::task::spawn_blocking(move || {
                        let manifest_basenames = EcosystemManifest::ALL
                            .iter()
                            .map(|manifest| manifest.basename())
                            .collect::<Vec<_>>();
                        pnpm_workspace::find_workspace_inventory(
                            &workspace_root,
                            &manifest_basenames,
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
        Ok(inventory
            .manifests(manifest.basename())
            .expect("every ecosystem manifest basename is inventoried"))
    }
}

#[cfg(test)]
mod tests;
