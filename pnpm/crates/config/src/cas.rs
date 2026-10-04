pub use pnpm_store_dir::{CAS_LOADER_FILENAME, CAS_MANIFEST_FILENAME};

use crate::{Config, NodeLinker};
use std::{collections::HashMap, path::Path};

impl Config {
    pub(super) fn apply_cas_layout(&mut self) {
        if self.node_linker != NodeLinker::Cas {
            return;
        }
        self.enable_global_virtual_store = true;
        if !self.explicit_settings.contains_key("modulesDir")
            && self.modules_dir
                .file_name()
                .is_some_and(|name| name == "node_modules")
        {
            self.modules_dir.set_file_name(".pnpm");
        }
    }

    pub fn add_cas_loader_env(&self, project: &Path, env: &mut HashMap<String, String>) {
        if self.node_linker != NodeLinker::Cas || self.virtual_store_only {
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
