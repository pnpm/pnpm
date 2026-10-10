//! Reading the global `config.yaml`.

use super::{
    ErrorKind, GLOBAL_CONFIG_YAML_FILENAME, LoadWorkspaceYamlError, Path, SystemEnv,
    WorkspaceSettings, fs, read_readable_settings, settings::parse_settings,
};

impl WorkspaceSettings {
    /// Read the global config.yaml at `<config_dir>/config.yaml`, if
    /// present.
    ///
    /// This file uses the same parser as `pnpm-workspace.yaml`, but a
    /// key-filter pass ([`Self::clear_workspace_only_fields`]) drops
    /// workspace-only knobs (`nodeLinker`, `hoist`, `lockfile`, ...)
    /// so they cannot be set globally.
    ///
    /// Returns `Ok(None)` when the file does not exist. Read or parse
    /// failures propagate.
    pub fn load_global(config_dir: &Path) -> Result<Option<Self>, LoadWorkspaceYamlError> {
        Self::load_global_skipping_unreadable(config_dir, false)
    }

    /// [`Self::load_global`], leaving out each top-level setting that does
    /// not parse on its own when `skip_unreadable` is set.
    pub fn load_global_skipping_unreadable(
        config_dir: &Path,
        skip_unreadable: bool,
    ) -> Result<Option<Self>, LoadWorkspaceYamlError> {
        let path = config_dir.join(GLOBAL_CONFIG_YAML_FILENAME);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(LoadWorkspaceYamlError::ReadFile { path, source }),
        };
        let read = parse_settings::<SystemEnv>;
        let mut settings =
            if skip_unreadable { read_readable_settings(&text, read) } else { read(&text) }
                .map_err(|source| LoadWorkspaceYamlError::ParseYaml {
                    path: path.clone(),
                    source,
                })?;
        settings.validate_registries()?;
        settings.validate_tasks()?;
        settings.validate_pipelines()?;
        settings.clear_workspace_only_fields();
        settings.warn_about_dropped_keys(&text, &path);
        Ok(Some(settings))
    }
}
