use miette::{IntoDiagnostic, Result, WrapErr};
use std::path::PathBuf;
use tokio::sync::OnceCell;

const IGNORED_DIRECTORY_BASENAMES: &[&str] =
    &[".git", ".pnpm", "node_modules", "target", ".venv", "venv", "__pycache__"];

#[derive(Clone, Copy)]
pub(crate) enum EcosystemManifest {
    Cargo,
    Python,
}

impl EcosystemManifest {
    const ALL: &[Self] = &[Self::Cargo, Self::Python];

    const fn basename(self) -> &'static str {
        match self {
            Self::Cargo => "Cargo.toml",
            Self::Python => "pyproject.toml",
        }
    }
}

/// Manifest paths available to every ecosystem participating in an install.
pub(crate) struct EcosystemWorkspaceInventory {
    workspace_root: PathBuf,
    managed_directories: Vec<PathBuf>,
    contents: OnceCell<pnpm_workspace::WorkspaceInventory>,
}

impl EcosystemWorkspaceInventory {
    pub(crate) fn new(workspace_root: PathBuf, config: &pnpm_config::Config) -> Self {
        let managed_directories = vec![
            config.store_dir.root().to_path_buf(),
            config.cache_dir.clone(),
            config.state_dir.clone(),
            config.modules_dir.clone(),
            config.virtual_store_dir.clone(),
            config.global_virtual_store_dir.clone(),
        ];
        Self { workspace_root, managed_directories, contents: OnceCell::new() }
    }

    pub(crate) async fn manifests(&self, manifest: EcosystemManifest) -> Result<&[PathBuf]> {
        let inventory = self
            .contents
            .get_or_try_init(|| {
                let workspace_root = self.workspace_root.clone();
                let managed_directories = self.managed_directories.clone();
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
                            &managed_directories,
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
