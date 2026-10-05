pub use pnpm_store_dir::{CAS_LOADER_FILENAME, CAS_MANIFEST_FILENAME};

use crate::{Config, NodeLinker};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// The modules layout the loaded linker overrode.
///
/// That linker forces `enableGlobalVirtualStore` on and points the modules
/// dir at `.pnpm`, so these are the values to put back.
#[derive(Debug, Clone)]
pub struct LoadedLayoutDefaults {
    pub modules_dir: PathBuf,
    pub enable_global_virtual_store: bool,
}

impl Config {
    pub(super) fn apply_cas_layout(&mut self) {
        if self.node_linker == NodeLinker::Loaded {
            self.loaded_layout_defaults.get_or_insert_with(|| LoadedLayoutDefaults {
                modules_dir: self.modules_dir.clone(),
                enable_global_virtual_store: self.enable_global_virtual_store,
            });
            self.enable_global_virtual_store = true;
            if !self.explicit_settings.contains_key("modulesDir") {
                self.modules_dir.set_file_name(".pnpm");
            }
        } else if let Some(defaults) = self.loaded_layout_defaults.take() {
            if !self.explicit_settings.contains_key("modulesDir") {
                self.modules_dir = defaults.modules_dir;
                if self.modules_dir
                    .file_name()
                    .is_some_and(|name| name == ".pnpm")
                {
                    self.modules_dir.set_file_name("node_modules");
                }
            }
            self.enable_global_virtual_store = self.explicit_settings
                .get("enableGlobalVirtualStore")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(defaults.enable_global_virtual_store);
        }
        self.follow_modules_dir_with_install_state_dir();
    }

    #[must_use]
    pub fn workspace_state_modules_dir(&self, root: &Path) -> std::path::PathBuf {
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
        let loader = self.lockfile_dir_for(project).join(CAS_LOADER_FILENAME);
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
