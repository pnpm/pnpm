use super::{IndexMap, LoadWorkspaceYamlError, RegistryEntry};
use crate::workspace_yaml::{WORKSPACE_MANIFEST_FILENAME, redact_registry_url};

/// Fail with [`LoadWorkspaceYamlError::RegistryEnvNotExpanded`] on the first
/// entry whose registry URL carries an unexpanded `${VAR}`.
pub fn refuse_env_placeholder_entries(
    entries: &IndexMap<String, RegistryEntry>,
    has_env_placeholder: impl Fn(&str) -> bool,
) -> Result<(), LoadWorkspaceYamlError> {
    match entries
        .iter()
        .find(|(key, entry)| has_env_placeholder(destination(key, entry)))
    {
        Some((key, _)) => Err(LoadWorkspaceYamlError::RegistryEnvNotExpanded {
            setting: format!("registries['{}']", redact_registry_url(key)),
            file: WORKSPACE_MANIFEST_FILENAME.to_string(),
        }),
        None => Ok(()),
    }
}

/// Drop every entry whose registry URL carries an unexpanded `${VAR}`.
pub fn retain_without_env_placeholders(
    entries: &mut IndexMap<String, RegistryEntry>,
    has_env_placeholder: impl Fn(&str) -> bool,
) {
    entries.retain(|key, entry| !has_env_placeholder(destination(key, entry)));
}

/// The registry URL of an entry: the value of a scope route, and the key of a
/// declaration.
fn destination<'entry>(key: &'entry str, entry: &'entry RegistryEntry) -> &'entry str {
    match entry {
        RegistryEntry::ScopeRoute(url) => url,
        RegistryEntry::Declaration(_) => key,
    }
}
