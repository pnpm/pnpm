use super::{BTreeMap, EnvVar, IndexMap, RegistryEntry, env_replace_lossy, placeholder_ranges};

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

/// A `${VAR}` / `${VAR:-fallback}` placeholder of a document, and the text
/// resolving it puts in its place.
pub(super) struct Placeholder {
    pub(super) range: std::ops::Range<usize>,
    pub(super) resolved: String,
}

/// The placeholders of `text` that may stand in for what a setting is written
/// as, in the order they appear.
///
/// A placeholder qualifies only when it resolves to a bare token — ASCII
/// letters, digits, `-`, `_`, and `.`. A variant name, a boolean, and a
/// number each are one; a URL, a path, an unresolved placeholder, and an
/// empty expansion are not. That bounds what resolving one can do twice over:
/// the text put back cannot spell yaml of its own, and it cannot name a
/// request destination.
pub(super) fn resolvable_placeholders<Sys: EnvVar>(text: &str) -> Vec<Placeholder> {
    placeholder_ranges(text)
        .into_iter()
        .filter_map(|range| {
            let (resolved, unresolved) = env_replace_lossy::<Sys>(&text[range.clone()]);
            (unresolved.is_empty() && is_bare_token(&resolved)).then_some(Placeholder {
                range,
                resolved,
            })
        })
        .collect()
}

/// `text` with the `selected` placeholders resolved and every other byte of
/// it left alone, so a scalar this did not resolve reaches the reader as the
/// file spells it.
pub(super) fn resolve_placeholders(
    text: &str,
    placeholders: &[Placeholder],
    selected: impl Fn(usize) -> bool,
) -> String {
    let mut resolved = String::with_capacity(text.len());
    let mut copied = 0;
    for (index, placeholder) in placeholders.iter().enumerate() {
        if !selected(index) {
            continue;
        }
        resolved.push_str(&text[copied..placeholder.range.start]);
        resolved.push_str(&placeholder.resolved);
        copied = placeholder.range.end;
    }
    resolved.push_str(&text[copied..]);
    resolved
}

/// `text` with every placeholder replaced by `null`, leaving the document the
/// file describes apart from the settings a placeholder decides.
pub(super) fn drop_placeholders(text: &str, placeholders: &[Placeholder]) -> String {
    let dropped: Vec<Placeholder> = placeholders
        .iter()
        .map(|placeholder| Placeholder {
            range: placeholder.range.clone(),
            resolved: "null".to_string(),
        })
        .collect();
    resolve_placeholders(text, &dropped, |_| true)
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
