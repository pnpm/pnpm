//! Reading the global `config.yaml`.

use super::{
    ErrorKind, GLOBAL_CONFIG_YAML_FILENAME, LoadWorkspaceYamlError, Path, SystemEnv,
    WorkspaceSettings, fs, read_readable_settings, select_settings, settings::parse_settings,
};
use crate::SettingsRead;

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
        Self::load_global_reading(config_dir, SettingsRead::All)
    }

    /// [`Self::load_global`], reading the settings `settings_read` selects.
    pub fn load_global_reading(
        config_dir: &Path,
        settings_read: SettingsRead,
    ) -> Result<Option<Self>, LoadWorkspaceYamlError> {
        let path = config_dir.join(GLOBAL_CONFIG_YAML_FILENAME);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(LoadWorkspaceYamlError::ReadFile { path, source }),
        };
        let read = parse_settings::<SystemEnv>;
        let selected = select_settings(&text, settings_read);
        let mut settings = if settings_read.skips_unreadable() {
            read_readable_settings(&selected, read)
        } else {
            read(&selected)
        }
        .map_err(|source| LoadWorkspaceYamlError::ParseYaml { path: path.clone(), source })?;
        settings.validate_sections(settings_read.skips_unreadable())?;
        settings.clear_workspace_only_fields();
        settings.warn_about_dropped_keys(&text, &path);
        Ok(Some(settings))
    }
}
