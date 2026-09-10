use super::{BTreeMap, EnvVar, RegistryEntry, env_replace_lossy};

/// Flatten a `noProxy` yaml scalar into the raw string form the `.npmrc`
/// spelling of the key would carry. `true` becomes the literal token the
/// `no-proxy` parser reads as "bypass every proxy"; anything that is
/// neither a string nor `true` becomes a token that reads as unset.
pub(super) fn no_proxy_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Bool(true) => "true".to_string(),
        serde_json::Value::String(value) => value.clone(),
        _ => "null".to_string(),
    }
}

pub(super) fn has_env_placeholder(value: &str) -> bool {
    value
        .match_indices("${")
        .any(|(start, _)| value[start + 2..].find('}').is_some_and(|end| end > 0))
}

pub(super) fn substitute_optional_string<Sys: EnvVar>(value: &mut Option<String>) {
    if let Some(value) = value {
        let (substituted, _) = env_replace_lossy::<Sys>(value);
        *value = substituted;
    }
}

pub(super) fn substitute_json_string<Sys: EnvVar>(value: &mut Option<serde_json::Value>) {
    if let Some(serde_json::Value::String(value)) = value {
        let (substituted, _) = env_replace_lossy::<Sys>(value);
        *value = substituted;
    }
}

pub(super) fn substitute_optional_string_map<Sys: EnvVar>(
    value: &mut Option<BTreeMap<String, String>>,
) {
    if let Some(value) = value {
        for map_value in value.values_mut() {
            let (substituted, _) = env_replace_lossy::<Sys>(map_value);
            *map_value = substituted;
        }
    }
}

/// Expands `${VAR}` in the half of each `registries` entry that carries the
/// request destination: the value of a scope route, the key of a declaration.
pub(super) fn substitute_registry_entries<Sys: EnvVar>(
    value: &mut Option<BTreeMap<String, RegistryEntry>>,
) {
    let Some(map) = value.take() else { return };
    *value = Some(
        map.into_iter()
            .map(|(key, entry)| match entry {
                RegistryEntry::ScopeRoute(url) => {
                    let (substituted, _) = env_replace_lossy::<Sys>(&url);
                    (key, RegistryEntry::ScopeRoute(substituted))
                }
                RegistryEntry::Declaration(declaration) => {
                    let (substituted, _) = env_replace_lossy::<Sys>(&key);
                    (substituted, RegistryEntry::Declaration(declaration))
                }
            })
            .collect(),
    );
}

pub(super) fn substitute_optional_inner_string<Sys: EnvVar>(value: &mut Option<Option<String>>) {
    if let Some(Some(value)) = value {
        let (substituted, _) = env_replace_lossy::<Sys>(value);
        *value = substituted;
    }
}

pub(super) fn normalize_registry_url(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}
