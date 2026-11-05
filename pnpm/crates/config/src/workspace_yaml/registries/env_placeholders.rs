use super::{
    DEFAULT_REGISTRY_SCOPE, IndexMap, LoadWorkspaceYamlError, RegistryEntry, python::PythonPattern,
};
use crate::workspace_yaml::{WORKSPACE_MANIFEST_FILENAME, redact_registry_url};

/// Drop the entries whose request destination carries an unexpanded `${VAR}`,
/// and return the settings dropped. The destination is the value of a scope
/// route and the key of a declaration, so which half is gated follows the
/// entry's shape.
///
/// Only an entry for the default registry may be dropped. Any other entry
/// routes packages that would otherwise resolve from the default registry, so
/// it fails with [`LoadWorkspaceYamlError::RegistryEnvNotExpanded`].
pub fn drop_env_placeholder_entries(
    entries: &mut IndexMap<String, RegistryEntry>,
    has_env_placeholder: impl Fn(&str) -> bool,
) -> Result<Vec<String>, LoadWorkspaceYamlError> {
    let mut dropped = Vec::new();
    for (key, entry) in entries.iter() {
        let destination = match entry {
            RegistryEntry::ScopeRoute(url) => url,
            RegistryEntry::Declaration(_) => key,
        };
        if !has_env_placeholder(destination) {
            continue;
        }
        let setting = format!("registries['{}']", redact_registry_url(key));
        if routes_non_default_packages(key, entry) {
            return Err(LoadWorkspaceYamlError::RegistryEnvNotExpanded {
                setting,
                file: WORKSPACE_MANIFEST_FILENAME.to_string(),
            });
        }
        dropped.push(setting);
    }
    entries.retain(|key, entry| match entry {
        RegistryEntry::ScopeRoute(url) => !has_env_placeholder(url),
        RegistryEntry::Declaration(_) => !has_env_placeholder(key),
    });
    Ok(dropped)
}

fn routes_non_default_packages(key: &str, entry: &RegistryEntry) -> bool {
    match entry {
        RegistryEntry::ScopeRoute(_) => key != "default",
        RegistryEntry::Declaration(declaration) => {
            declaration.prefixes
                .iter()
                .next()
                .is_some()
                || declaration.scopes
                    .iter()
                    .flatten()
                    .any(|scope| scope != DEFAULT_REGISTRY_SCOPE)
                || declaration.packages
                    .iter()
                    .flatten()
                    .any(|pattern| {
                        !PythonPattern::parse(pattern).is_ok_and(|pattern| pattern.is_default())
                    })
        }
    }
}
