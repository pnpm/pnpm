use super::{BTreeMap, EnvVar, IndexMap, RegistryEntry, env_replace_lossy};

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
        .any(|(start, _)| {
            value[start + 2..]
                .find('}')
                .is_some_and(|end| end > 0)
        })
}

/// The settings whose value names where a request goes. A
/// repository-controlled file must not resolve an environment variable into
/// one, so [`expand_typed_placeholders`] leaves them to
/// [`WorkspaceSettings::substitute_env_untrusted`](super::WorkspaceSettings::substitute_env_untrusted),
/// which drops the placeholder instead of expanding it.
const REQUEST_DESTINATION_KEYS: &[&str] = &[
    "httpProxy",
    "httpsProxy",
    "namedRegistries",
    "noProxy",
    "noproxy",
    "pnprServer",
    "proxy",
    "registries",
    "registry",
];

/// Resolve the `${VAR}` / `${VAR:-fallback}` placeholders a setting that is
/// not a string cannot carry as text, reporting whether any were resolved.
///
/// `nodeLinker: ${PNPM_NODE_LINKER:-isolated}` names a variant and
/// `ignoreScripts: ${CI:-false}` a boolean, but serde sees the literal
/// placeholder in both, so a document holding one has to be read again with
/// the placeholders already gone.
///
/// Only an expansion that is a bare token — ASCII letters, digits, `-`, `_`,
/// and `.` — replaces the text it came from. A variant name, a boolean, and
/// a number each are one; a URL, a path, an unresolved placeholder, and an
/// empty expansion are not. Those are left as written for
/// [`WorkspaceSettings::substitute_env_trusted`](super::WorkspaceSettings::substitute_env_trusted)
/// and its untrusted counterpart, which resolve them once the settings are
/// typed and know which layer the file came from.
pub(super) fn expand_typed_placeholders<Sys: EnvVar>(document: &mut serde_json::Value) -> bool {
    let serde_json::Value::Object(settings) = document else { return false };
    let mut expanded = false;
    for (key, value) in settings {
        if REQUEST_DESTINATION_KEYS.contains(&key.as_str()) {
            continue;
        }
        expanded |= expand_tokens::<Sys>(value);
    }
    expanded
}

fn expand_tokens<Sys: EnvVar>(value: &mut serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => match expanded_token::<Sys>(text) {
            Some(token) => {
                *value = scalar_value(token);
                true
            }
            None => false,
        },
        serde_json::Value::Array(items) => {
            let mut expanded = false;
            for item in items {
                expanded |= expand_tokens::<Sys>(item);
            }
            expanded
        }
        serde_json::Value::Object(entries) => {
            let mut expanded = false;
            for entry in entries.values_mut() {
                expanded |= expand_tokens::<Sys>(entry);
            }
            expanded
        }
        _ => false,
    }
}

fn expanded_token<Sys: EnvVar>(text: &str) -> Option<String> {
    if !has_env_placeholder(text) {
        return None;
    }
    let (expanded, unresolved) = env_replace_lossy::<Sys>(text);
    (unresolved.is_empty() && is_bare_token(&expanded)).then_some(expanded)
}

/// Read an expansion as the yaml scalar the file would have carried had the
/// value been written out, so `${CI:-false}` reaches a boolean setting as a
/// boolean rather than as the text of one.
fn scalar_value(token: String) -> serde_json::Value {
    match serde_saphyr::from_str::<serde_json::Value>(&token) {
        Ok(value) => value,
        Err(_) => serde_json::Value::String(token),
    }
}

fn is_bare_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
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
    value: &mut Option<IndexMap<String, RegistryEntry>>,
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
