use super::{BTreeMap, EnvVar, IndexMap, RegistryEntry, SentinelPlaceholders, env_replace_lossy};
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
    sentinels: SentinelPlaceholders,
    pub(super) items: Vec<Placeholder>,
    pub(super) resolvable: Vec<usize>,
}

/// Collect the `${...}` placeholders of `text` in source order, returning
/// `None` when `text` contains none.
///
/// Only a placeholder that resolves without missing variables to a bare token
/// (`[A-Za-z0-9_.-]+`) and is not inside a YAML anchor/alias token (`&` or
/// `*`) receives `resolved: Some(...)`. Every placeholder also receives a
/// bare-token `sentinel` (`{prefix}{index}_`) whose prefix cannot appear in
/// `text` (even after YAML double-quoted unescaping) or in any resolved value,
/// so YAML syntax indicators inside `${...}` (such as `{`, `}`, or `: `) do not
/// prevent a free-text setting from parsing with its placeholder left
/// unexpanded.
pub(super) fn document_placeholders<Sys: EnvVar>(text: &str) -> Option<DocumentPlaceholders> {
    let ranges = SentinelPlaceholders::single_line_ranges(text);
    if ranges.is_empty() {
        return None;
    }
    let resolved_values = resolve_bare_tokens::<Sys>(text, &ranges);
    let sentinels = SentinelPlaceholders::with_extra(
        text,
        ranges.clone(),
        resolved_values.iter().filter_map(Option::as_deref),
    )?;
    let mut items = Vec::with_capacity(ranges.len());
    let mut resolvable = Vec::new();
    for (index, (range, resolved)) in ranges
        .into_iter()
        .zip(resolved_values)
        .enumerate()
    {
        if resolved.is_some() {
            resolvable.push(index);
        }
        items.push(Placeholder { range, resolved, sentinel: sentinels.sentinel(index) });
    }
    Some(DocumentPlaceholders { sentinels, items, resolvable })
}

fn resolve_bare_tokens<Sys: EnvVar>(text: &str, ranges: &[Range<usize>]) -> Vec<Option<String>> {
    let mut in_anchor_or_alias = false;
    let mut cursor = 0;
    let mut resolved = Vec::with_capacity(ranges.len());
    for range in ranges {
        advance_anchor_or_alias_state(&mut in_anchor_or_alias, &text[cursor..range.start]);
        cursor = range.end;
        resolved.push(
            (!in_anchor_or_alias)
                .then(|| resolve_bare_token::<Sys>(&text[range.clone()]))
                .flatten(),
        );
    }
    resolved
}

fn advance_anchor_or_alias_state(in_anchor_or_alias: &mut bool, segment: &str) {
    for character in segment.chars() {
        if matches!(character, '&' | '*') {
            *in_anchor_or_alias = true;
        } else if matches!(character, ' ' | '\t' | '\r' | '\n' | ',' | '[' | ']' | '{' | '}') {
            *in_anchor_or_alias = false;
        }
    }
}

fn resolve_bare_token<Sys: EnvVar>(placeholder: &str) -> Option<String> {
    let (expanded, unresolved) = env_replace_lossy::<Sys>(placeholder);
    (unresolved.is_empty() && is_bare_token(&expanded)).then_some(expanded)
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
        self.sentinels.may_contain_sentinel(value)
    }

    /// Replace every `{sentinel_prefix}{index}_` token in `value` back with
    /// the original `&original_text[range]` slice.
    pub(super) fn restore(&self, original_text: &str, value: &str) -> String {
        self.sentinels.restore(original_text, value)
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
