pub use pnpm_store_dir::{CAS_LOADER_FILENAME, CAS_MANIFEST_FILENAME};

use crate::{Config, NodeLinker};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

impl Config {
    pub(super) fn apply_cas_layout(&mut self) {
        if self.node_linker == NodeLinker::Loaded {
            self.global_virtual_store_before_loaded.get_or_insert(self.enable_global_virtual_store);
            self.enable_global_virtual_store = true;
        } else if let Some(global_store) = self
            .global_virtual_store_before_loaded
            .take()
        {
            self.enable_global_virtual_store = self.explicit_settings
                .get("enableGlobalVirtualStore")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(global_store);
        }
        self.follow_modules_dir_with_install_state_dir();
    }

    /// The directory of the install rooted at `install_root` that holds
    /// its store manifest and loader.
    #[must_use]
    pub fn store_loader_dir(&self, install_root: &Path) -> PathBuf {
        self.project_modules_dir(install_root, None).join(".pnpm")
    }

    #[must_use]
    pub fn workspace_state_modules_dir(&self, root: &Path) -> PathBuf {
        if self.node_linker == NodeLinker::Loaded {
            self.modules_dir.clone()
        } else {
            root.join("node_modules")
        }
    }

    pub fn add_cas_loader_env(&self, project: &Path, env: &mut HashMap<String, String>) {
        if self.node_linker != NodeLinker::Loaded || self.virtual_store_only {
            return;
        }
        let loader = self
            .store_loader_dir(self.lockfile_dir_for(project))
            .join(CAS_LOADER_FILENAME);
        let url = url::Url::from_file_path(&loader).expect("the install root is absolute");
        let option = format!("--import={url}");
        let previous = env
            .get("NODE_OPTIONS")
            .cloned()
            .or_else(|| std::env::var("NODE_OPTIONS").ok())
            .unwrap_or_default();
        if !previous
            .split_whitespace()
            .any(|part| part == option)
        {
            env.insert(
                "NODE_OPTIONS".to_string(),
                format!("{previous} {option}").trim().to_string(),
            );
        }
    }
}
