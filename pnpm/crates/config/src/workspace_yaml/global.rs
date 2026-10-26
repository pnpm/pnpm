//! Reading the global `config.yaml`.

use super::{
    DroppedKeys, ErrorKind, GLOBAL_CONFIG_YAML_FILENAME, IgnoredAny, IndexMap,
    LoadWorkspaceYamlError, MachineSettings, Path, SCHEMA_DIRECTIVE_KEY, SystemEnv,
    TrustedSettings, fs, parse_settings, read_readable_settings, redact_and_sanitize,
    select_settings,
};
use crate::SettingsRead;
use serde::de::DeserializeOwned;

impl TrustedSettings {
    /// Read the global config.yaml at `<config_dir>/config.yaml`, if
    /// present.
    ///
    /// This file uses the same parser as `pnpm-workspace.yaml`, but a
    /// key-filter pass
    /// ([`WorkspaceSettings::clear_workspace_only_fields`](super::WorkspaceSettings::clear_workspace_only_fields))
    /// drops workspace-only knobs (`nodeLinker`, `hoist`, `lockfile`, ...)
    /// so they cannot be set globally.
    ///
    /// The file is read once per half rather than through one
    /// `#[serde(flatten)]` struct: flattening buffers each value untyped,
    /// which loses the YAML scalar-to-string reading a setting like
    /// `tag: 1.0` depends on. The machine half is read only from a file that
    /// may name one of its settings.
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
        let selected = select_settings(&text, settings_read);
        let mut settings = Self {
            settings: read_global(&selected, &path, settings_read)?,
            machine: if MachineSettings::may_be_named_in(&selected) {
                read_global(&selected, &path, settings_read)?
            } else {
                MachineSettings::default()
            },
        };
        settings.settings.validate_sections(settings_read.skips_unreadable())?;
        settings.settings.clear_workspace_only_fields();
        settings.warn_about_dropped_keys(&text, &path);
        Ok(Some(settings))
    }

    /// Warn about the keys of the global `config.yaml` that never reach the
    /// settings, in the messages pnpm emits for that file.
    ///
    /// What survived is read back off `self` rather than off a second list of
    /// key names, which would drift from the structs: a key serde did not
    /// recognize is absent from the serialized settings, and one
    /// [`crate::WorkspaceSettings::clear_workspace_only_fields`] zeroed is null there.
    ///
    /// A dropped camelCase key pnpm's `isConfigFileKey` accepts stays silent:
    /// pnpm honors it in this file, so the fix is to honor it too, and until
    /// then a warning would diverge from pnpm's output on the same file.
    fn warn_about_dropped_keys(&self, text: &str, path: &Path) {
        let Ok(document) = serde_saphyr::from_str::<IndexMap<String, Option<IgnoredAny>>>(text)
        else {
            return;
        };
        let Ok(serde_json::Value::Object(kept)) = self.to_json() else {
            return;
        };

        let mut dropped = DroppedKeys::default();
        for key in document
            .iter()
            .filter(|(_, value)| value.is_some())
            .map(|(key, _)| key)
        {
            if key == SCHEMA_DIRECTIVE_KEY
                || matches!(kept.get(key), Some(value) if !value.is_null())
            {
                continue;
            }
            // The key comes from a file the machine's user controls, but the
            // same rendering serves the project file, so it is sanitized here
            // too rather than only where it must be.
            dropped.classify(&redact_and_sanitize(key));
        }
        dropped.warn(path);
    }
}

fn read_global<Settings: DeserializeOwned>(
    text: &str,
    path: &Path,
    settings_read: SettingsRead,
) -> Result<Settings, LoadWorkspaceYamlError> {
    let read = parse_settings::<SystemEnv, Settings>;
    if settings_read.skips_unreadable() { read_readable_settings(text, read) } else { read(text) }
        .map_err(|source| LoadWorkspaceYamlError::ParseYaml { path: path.to_path_buf(), source })
}
