use super::{
    DEFAULT_REGISTRY_SCOPE, IndexMap, LoadWorkspaceYamlError, RegistryEntry, python::PythonPattern,
};
use crate::workspace_yaml::{WORKSPACE_MANIFEST_FILENAME, redact_registry_url};

/// Drop the entries whose request destination carries an unexpanded `${VAR}`,
/// and return the settings dropped.
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
        if !has_env_placeholder(destination(key, entry)) {
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
    retain_without_env_placeholders(entries, has_env_placeholder);
    Ok(dropped)
}

/// Drop every entry whose request destination carries an unexpanded `${VAR}`,
/// whatever it routes.
pub fn retain_without_env_placeholders(
    entries: &mut IndexMap<String, RegistryEntry>,
    has_env_placeholder: impl Fn(&str) -> bool,
) {
    entries.retain(|key, entry| !has_env_placeholder(destination(key, entry)));
}

/// The value of a scope route, and the key of a declaration.
fn destination<'entry>(key: &'entry str, entry: &'entry RegistryEntry) -> &'entry str {
    match entry {
        RegistryEntry::ScopeRoute(url) => url,
        RegistryEntry::Declaration(_) => key,
    }
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
