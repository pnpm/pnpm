use super::{
    BTreeMap, EnvVar, IndexMap, LoadWorkspaceYamlError, RegistryEntry, SentinelPlaceholders,
    env_replace_lossy,
};
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
/// bare-token expansion when it has one, and a unique style-probing sentinel
/// used while probing whether the target setting accepts free text.
pub(super) struct Placeholder {
    pub(super) range: Range<usize>,
    /// Whether the placeholder sits where a value can stand, outside a YAML
    /// anchor, alias, or tag token and outside an incomplete escape.
    pub(super) in_value: bool,
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
/// (`[A-Za-z0-9_.-]+`) outside YAML anchor/alias/tag tokens (`&`, `*`, `!`) and
/// incomplete `\x`/`\u`/`\U` hex escapes receives `resolved: Some(...)`. Every
/// placeholder also receives a style-probing `sentinel` whose prefix cannot
/// appear in `text` (even after YAML double-quoted unescaping) or in any
/// resolved value, so YAML syntax indicators inside `${...}` (such as `{`, `}`,
/// or `: `) do not prevent a free-text setting from parsing with its
/// placeholder left unexpanded.
pub(super) fn document_placeholders<Sys: EnvVar>(text: &str) -> Option<DocumentPlaceholders> {
    let ranges = SentinelPlaceholders::single_line_ranges(text);
    if ranges.is_empty() {
        return None;
    }
    let in_values = value_positions(text, &ranges);
    let resolved_values: Vec<Option<String>> = ranges
        .iter()
        .zip(&in_values)
        .map(|(range, &in_value)| {
            in_value
                .then(|| resolve_bare_token::<Sys>(&text[range.clone()]))
                .flatten()
        })
        .collect();
    let sentinels = SentinelPlaceholders::with_extra(
        text,
        ranges.clone(),
        resolved_values.iter().filter_map(Option::as_deref),
    )?;
    let mut items = Vec::with_capacity(ranges.len());
    let mut resolvable = Vec::new();
    for (index, ((range, in_value), resolved)) in ranges
        .into_iter()
        .zip(in_values)
        .zip(resolved_values)
        .enumerate()
    {
        if resolved.is_some() {
            resolvable.push(index);
        }
        items.push(Placeholder { range, in_value, resolved, sentinel: sentinels.sentinel(index) });
    }
    Some(DocumentPlaceholders { sentinels, items, resolvable })
}

fn value_positions(text: &str, ranges: &[Range<usize>]) -> Vec<bool> {
    let mut in_meta_token = false;
    let mut cursor = 0;
    ranges
        .iter()
        .map(|range| {
            advance_yaml_meta_state(&mut in_meta_token, &text[cursor..range.start]);
            cursor = range.end;
            !in_meta_token && !has_incomplete_hex_escape_before(&text[..range.start])
        })
        .collect()
}

fn advance_yaml_meta_state(in_meta_token: &mut bool, segment: &str) {
    for character in segment.chars() {
        if matches!(character, '&' | '*' | '!') {
            *in_meta_token = true;
        } else if matches!(character, ' ' | '\t' | '\r' | '\n' | ',' | '[' | ']' | '{' | '}') {
            *in_meta_token = false;
        }
    }
}

fn has_incomplete_hex_escape_before(prefix: &str) -> bool {
    let bytes = prefix.as_bytes();
    let hex_len = bytes
        .iter()
        .rev()
        .take(8)
        .take_while(|byte| byte.is_ascii_hexdigit())
        .count();
    let Some(escape_pos) = bytes.len().checked_sub(hex_len + 1) else {
        return false;
    };
    let required_digits = match bytes[escape_pos] {
        b'x' => 2,
        b'u' => 4,
        b'U' => 8,
        _ => return false,
    };
    if hex_len >= required_digits {
        return false;
    }
    let backslashes = bytes[..escape_pos]
        .iter()
        .rev()
        .take_while(|&&byte| byte == b'\\')
        .count();
    backslashes % 2 == 1
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

    /// Restore every sentinel token in `value` back to its `${...}` text
    /// decoded according to the enclosing YAML scalar style.
    pub(super) fn try_restore(&self, original_text: &str, value: &str) -> Option<String> {
        self.sentinels.try_restore(original_text, value)
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

/// What a substitution pass does with a `${VAR}` placeholder that has no
/// value and no fallback.
#[derive(Clone, Copy)]
enum OnUnresolved {
    /// Expand it to an empty string and fail the pass once it is done.
    Fail,
    /// Expand it to an empty string.
    ExpandEmpty,
    /// Leave its setting out.
    LeaveOut,
}

/// The state of one substitution pass over unresolved placeholders.
pub(super) struct UnresolvedPlaceholders {
    on_unresolved: OnUnresolved,
    first: Option<String>,
}

impl UnresolvedPlaceholders {
    pub(super) fn fail() -> Self {
        Self { on_unresolved: OnUnresolved::Fail, first: None }
    }

    pub(super) fn expand_empty() -> Self {
        Self { on_unresolved: OnUnresolved::ExpandEmpty, first: None }
    }

    pub(super) fn leave_out() -> Self {
        Self { on_unresolved: OnUnresolved::LeaveOut, first: None }
    }

    /// [`LoadWorkspaceYamlError::ConfigUnresolvedEnvVar`] for the first
    /// placeholder a [`Self::fail`] pass could not resolve.
    pub(super) fn into_result(self) -> Result<(), LoadWorkspaceYamlError> {
        self.first.map_or(Ok(()), |var| Err(LoadWorkspaceYamlError::ConfigUnresolvedEnvVar { var }))
    }

    /// `text` expanded, or `None` when its setting is to be left out.
    fn replace<Sys: EnvVar>(&mut self, text: &str) -> Option<String> {
        let (substituted, unresolved) = env_replace_lossy::<Sys>(text);
        let Some(var) = unresolved.into_iter().next() else { return Some(substituted) };
        match self.on_unresolved {
            OnUnresolved::Fail => {
                self.first.get_or_insert(var);
                Some(substituted)
            }
            OnUnresolved::ExpandEmpty => Some(substituted),
            OnUnresolved::LeaveOut => None,
        }
    }
}

pub(super) fn substitute_optional_string<Sys: EnvVar>(
    value: &mut Option<String>,
    unresolved: &mut UnresolvedPlaceholders,
) {
    if let Some(text) = value.as_deref() {
        *value = unresolved.replace::<Sys>(text);
    }
}

pub(super) fn substitute_json_string<Sys: EnvVar>(
    value: &mut Option<serde_json::Value>,
    unresolved: &mut UnresolvedPlaceholders,
) {
    if let Some(serde_json::Value::String(text)) = value {
        *value = unresolved.replace::<Sys>(text).map(serde_json::Value::String);
    }
}

pub(super) fn substitute_optional_string_map<Sys: EnvVar>(
    value: &mut Option<BTreeMap<String, String>>,
    unresolved: &mut UnresolvedPlaceholders,
) {
    let Some(map) = value.take() else { return };
    *value = Some(
        map.into_iter()
            .filter_map(|(key, text)| {
                unresolved
                    .replace::<Sys>(&text)
                    .map(|substituted| (key, substituted))
            })
            .collect(),
    );
}

/// Expands `${VAR}` in the half of each `registries` entry that carries the
/// request destination: the value of a scope route, the key of a declaration.
pub(super) fn substitute_registry_entries<Sys: EnvVar>(
    value: &mut Option<IndexMap<String, RegistryEntry>>,
    unresolved: &mut UnresolvedPlaceholders,
) {
    let Some(map) = value.take() else { return };
    *value = Some(
        map.into_iter()
            .filter_map(|(key, entry)| match entry {
                RegistryEntry::ScopeRoute(url) => unresolved
                    .replace::<Sys>(&url)
                    .map(|url| (key, RegistryEntry::ScopeRoute(url))),
                RegistryEntry::Declaration(declaration) => unresolved
                    .replace::<Sys>(&key)
                    .map(|key| (key, RegistryEntry::Declaration(declaration))),
            })
            .collect(),
    );
}

pub(super) fn substitute_optional_inner_string<Sys: EnvVar>(
    value: &mut Option<Option<String>>,
    unresolved: &mut UnresolvedPlaceholders,
) {
    if let Some(Some(text)) = value {
        *value = unresolved.replace::<Sys>(text).map(Some);
    }
}
