use super::{
    LoadWorkspaceYamlError, WorkspaceSettings, parse_settings, read_readable_settings,
    select_settings,
};
use crate::{
    SettingsRead, api::EnvVar, known_settings::is_known_setting_key, naming_cases::is_camel_case,
};
use pnpm_workspace::{
    Delegation, IncludeError, IncludeSource, SettingSlot, WorkspaceIncludes,
    WorkspaceManifestDocument,
};
use std::path::Path;

/// Read `<dir>/pnpm-workspace.yaml` with its local includes. `Ok(None)` only
/// when the file does not exist.
pub(crate) fn read_workspace_document(
    dir: &Path,
) -> Result<Option<WorkspaceManifestDocument>, LoadWorkspaceYamlError> {
    pnpm_workspace::read_workspace_manifest_document(dir)
        .map_err(|error| match error {
            IncludeError::ReadRoot { path, source } => {
                LoadWorkspaceYamlError::ReadFile { path, source }
            }
            error => LoadWorkspaceYamlError::Include(error),
        })
}

/// Check that the include index delegates only settings pnpm knows.
fn check_delegations(includes: &WorkspaceIncludes) -> Result<(), LoadWorkspaceYamlError> {
    for include in &includes.includes {
        if let Delegation::Listed(slots) = &include.delegation {
            check_setting_names(&include.source.to_string(), slots)?;
        }
    }
    Ok(())
}

/// Read each local source on its own, so a source's error names its file.
fn read_fragments(
    includes: &WorkspaceIncludes,
    read: impl Fn(&str, &Path) -> Result<WorkspaceSettings, LoadWorkspaceYamlError>,
) -> Result<(), LoadWorkspaceYamlError> {
    for (&position, text) in &includes.fragments {
        if let IncludeSource::Local { path, .. } = &includes.includes[position].source {
            read(text, path)?;
        }
    }
    Ok(())
}

fn check_setting_names(spec: &str, slots: &[SettingSlot]) -> Result<(), LoadWorkspaceYamlError> {
    let unknown = slots
        .iter()
        .find_map(|slot| match slot {
            SettingSlot::Setting(name) if !is_camel_case(name) || !is_known_setting_key(name) => {
                Some(name)
            }
            _ => None,
        });
    match unknown {
        Some(setting) => Err(LoadWorkspaceYamlError::UnknownDelegatedSetting {
            spec: spec.to_string(),
            setting: setting.clone(),
        }),
        None => Ok(()),
    }
}

impl WorkspaceSettings {
    /// The settings of a workspace manifest with its local includes merged
    /// in. Each local source is read on its own first, so an error names the
    /// file it is in.
    pub(crate) fn read_document(
        document: WorkspaceManifestDocument,
        settings_read: SettingsRead,
        read: impl Fn(&str, &Path) -> Result<Self, LoadWorkspaceYamlError>,
    ) -> Result<Self, LoadWorkspaceYamlError> {
        let WorkspaceManifestDocument { path, text, includes } = document;
        // The settings an include contributes decide nothing about the pin.
        let includes = includes.filter(|_| settings_read != SettingsRead::PinDecision);
        if let Some(includes) = &includes {
            check_delegations(includes)?;
            // Read leniently below, like the merged settings, so a command
            // that repairs a setting can still load them.
            if !settings_read.skips_unreadable() {
                read_fragments(includes, &read)?;
            }
        }
        let read_merged = |text: &str| read(text, &path);
        let text = select_settings(&text, settings_read);
        let mut settings = if settings_read.skips_unreadable() {
            read_readable_settings(&text, read_merged)
        } else {
            read_merged(&text)
        }?;
        settings.collect_key_issues(&text);
        settings.includes = includes;
        Ok(settings)
    }

    /// The settings of a config dependency's config entry at `path`, read and
    /// validated the way a project's `pnpm-workspace.yaml` is.
    pub fn read_config_dependency_entry<Sys: EnvVar>(
        text: &str,
        path: &Path,
    ) -> Result<Self, LoadWorkspaceYamlError> {
        let mut settings = parse_settings::<Sys, Self>(text)
            .map_err(|source| LoadWorkspaceYamlError::ParseYaml {
                path: path.to_path_buf(),
                source,
            })?;
        settings.validate_registries()?;
        settings.reject_repo_controlled_trust_material(path)?;
        settings.substitute_env_untrusted::<Sys>();
        Ok(settings)
    }
}
