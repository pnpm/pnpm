use super::{BTreeMap, EnvVar, IndexMap, RegistryEntry, env_replace_lossy, placeholder_ranges};
use std::ops::Range;

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

/// A `${VAR}` / `${VAR:-fallback}` placeholder in a YAML document, its
/// bare-token expansion when it has one, and a unique bare-token sentinel used
/// while probing whether the target setting accepts free text.
pub(super) struct Placeholder {
    pub(super) range: Range<usize>,
    pub(super) resolved: Option<String>,
    pub(super) sentinel: String,
}

/// All unescaped `${...}` placeholders in a YAML document, paired with a
/// sentinel prefix absent from both the source text and every resolved value.
pub(super) struct DocumentPlaceholders {
    sentinel_prefix: String,
    pub(super) items: Vec<Placeholder>,
    pub(super) resolvable: Vec<usize>,
}

/// Collect the `${...}` placeholders of `text` in source order, returning
/// `None` when `text` contains none.
///
/// Only a placeholder that resolves without missing variables to a bare token
/// (`[A-Za-z0-9_.-]+`) and is not preceded by a YAML anchor/alias sigil (`&`
/// or `*`) receives `resolved: Some(...)`. Every placeholder also receives a
/// bare-token `sentinel` (`{prefix}{index}_`) so YAML syntax indicators inside
/// `${...}` (such as `{`, `}`, or `: `) do not prevent a free-text setting
/// from parsing with its placeholder left unexpanded.
pub(super) fn document_placeholders<Sys: EnvVar>(text: &str) -> Option<DocumentPlaceholders> {
    let ranges = placeholder_ranges(text);
    if ranges.is_empty() {
        return None;
    }
    let raw_items: Vec<(Range<usize>, Option<String>)> = ranges
        .into_iter()
        .map(|range| {
            let resolved = (!text[..range.start].ends_with(['&', '*']))
                .then(|| env_replace_lossy::<Sys>(&text[range.clone()]))
                .and_then(|(expanded, unresolved)| {
                    (unresolved.is_empty() && is_bare_token(&expanded)).then_some(expanded)
                });
            (range, resolved)
        })
        .collect();
    let sentinel_prefix = unique_sentinel_prefix(text, &raw_items);
    let mut items = Vec::with_capacity(raw_items.len());
    let mut resolvable = Vec::new();
    for (index, (range, resolved)) in raw_items.into_iter().enumerate() {
        if resolved.is_some() {
            resolvable.push(index);
        }
        let sentinel = format!("{sentinel_prefix}{index}_");
        items.push(Placeholder { range, resolved, sentinel });
    }
    Some(DocumentPlaceholders { sentinel_prefix, items, resolvable })
}

impl DocumentPlaceholders {
    /// Render `text` replacing each placeholder with `replacement(index, item)`.
    pub(super) fn render<'out>(
        &'out self,
        text: &'out str,
        replacement: impl Fn(usize, &'out Placeholder) -> &'out str,
    ) -> String {
        let mut rendered = String::with_capacity(text.len());
        let mut copied = 0;
        for (index, placeholder) in self.items.iter().enumerate() {
            rendered.push_str(&text[copied..placeholder.range.start]);
            rendered.push_str(replacement(index, placeholder));
            copied = placeholder.range.end;
        }
        rendered.push_str(&text[copied..]);
        rendered
    }

    pub(super) fn may_contain_sentinel(&self, value: &str) -> bool {
        value.contains(&self.sentinel_prefix)
    }

    /// Replace every `{sentinel_prefix}{index}_` token in `value` back with
    /// the original `&original_text[range]` slice.
    pub(super) fn restore(&self, original_text: &str, value: &str) -> String {
        let mut restored = String::with_capacity(value.len());
        let mut cursor = 0;
        while let Some(offset) = value[cursor..].find(&self.sentinel_prefix) {
            let start = cursor + offset;
            restored.push_str(&value[cursor..start]);
            let after_prefix = start + self.sentinel_prefix.len();
            if let Some((raw_placeholder, end)) =
                self.parse_sentinel_at(original_text, value, after_prefix)
            {
                restored.push_str(raw_placeholder);
                cursor = end;
            } else {
                restored.push_str(&self.sentinel_prefix);
                cursor = after_prefix;
            }
        }
        restored.push_str(&value[cursor..]);
        restored
    }

    fn parse_sentinel_at<'text>(
        &self,
        original_text: &'text str,
        value: &str,
        after_prefix: usize,
    ) -> Option<(&'text str, usize)> {
        let digits_len = value[after_prefix..].find('_')?;
        let index: usize = value[after_prefix..after_prefix + digits_len].parse().ok()?;
        let placeholder = self.items.get(index)?;
        Some((&original_text[placeholder.range.clone()], after_prefix + digits_len + 1))
    }
}

fn unique_sentinel_prefix(text: &str, raw_items: &[(Range<usize>, Option<String>)]) -> String {
    let mut counter = 0_u32;
    loop {
        let candidate = format!("pnpmplaceholder{counter}_");
        let collides = text.contains(&candidate)
            || raw_items
                .iter()
                .filter_map(|(_, resolved)| resolved.as_deref())
                .any(|resolved| resolved.contains(&candidate));
        if !collides {
            return candidate;
        }
        counter += 1;
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
