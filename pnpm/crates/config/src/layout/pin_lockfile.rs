use super::Config;
use std::path::Path;

impl Config {
    /// Pin [`lockfile_dir`] to `dir` and re-anchor following paths unless
    /// `dir` is the default lockfile directory.
    ///
    /// `dir` is normalized first: importer ids are a lexical path diff
    /// against it, so an unnormalized `<workspace>/..` would not name the
    /// project it points at.
    ///
    /// [`lockfile_dir`]: Self::lockfile_dir
    pub fn pin_lockfile_dir(&mut self, dir: &Path, project_dir: &Path) {
        self.configured_lockfile_dir = Some(dir.to_path_buf());
        let dir = pnpm_fs::lexical_normalize(dir);
        if self.lockfile_dir_is_default(&dir, project_dir) {
            self.clear_lockfile_dir_pin(project_dir);
            return;
        }
        self.anchor_lockfile_paths(&dir);
        self.lockfile_dir = Some(dir);
    }

    fn clear_lockfile_dir_pin(&mut self, project_dir: &Path) {
        let Some(previous_lockfile_dir) = self.lockfile_dir.take() else {
            return;
        };
        let configured_virtual_store_dir = self.configured_virtual_store_dir.clone();
        let raw_virtual_store_dir = self.explicit_settings
            .get("virtualStoreDir")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let paths_dir = self.workspace_dir
            .as_deref()
            .unwrap_or(project_dir)
            .to_path_buf();
        self.anchor_lockfile_paths(&paths_dir);
        if self.enable_global_virtual_store {
            return;
        }
        if let (Some(configured), Some(raw)) = (configured_virtual_store_dir, raw_virtual_store_dir)
        {
            let raw_path = Path::new(&raw);
            let was_lockfile_relative = pnpm_fs::lexical_normalize(&configured)
                == pnpm_fs::lexical_normalize(&previous_lockfile_dir.join(raw_path));
            let dir = if was_lockfile_relative { paths_dir.join(raw_path) } else { configured };
            self.set_virtual_store_dir(dir);
        }
    }
}
