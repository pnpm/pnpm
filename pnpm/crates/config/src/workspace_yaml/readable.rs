use super::{Deserialize, Deserializer, IgnoredAny, IndexMap, WorkspaceSettings};
use crate::{PIN_DECISION_SETTINGS, SettingsRead};
use serde::de::{MapAccess, Visitor};
use serde_saphyr::Spanned;
use std::{borrow::Cow, fmt, fmt::Write as _, marker::PhantomData, ops::Range};

/// `text` with only the top-level settings `settings_read` reads.
///
/// [`SettingsRead::PinDecision`] keeps the blocks of the
/// [`PIN_DECISION_SETTINGS`] and blanks the rest, so what is left keeps its
/// lines.
pub(crate) fn select_settings(text: &str, settings_read: SettingsRead) -> Cow<'_, str> {
    if settings_read != SettingsRead::PinDecision {
        return Cow::Borrowed(text);
    }
    let Some(keys) = top_level_keys::<String>(text) else {
        return Cow::Owned(rewrite_selected_settings(text));
    };
    let key_lines: Vec<usize> = keys
        .iter()
        .map(|(line, _)| *line)
        .collect();
    let mut lines: Vec<&str> = text.split_inclusive('\n').collect();
    let line_count = lines.len();
    for ((_, name), setting) in keys
        .iter()
        .zip(setting_lines(&key_lines, line_count))
    {
        if !PIN_DECISION_SETTINGS.contains(&name.as_str()) {
            lines[setting].fill("\n");
        }
    }
    Cow::Owned(lines.concat())
}

/// Read `text` with `read`, leaving out each top-level setting that `read`
/// rejects on its own.
///
/// A newer pnpm can give a setting a shape or a value this one rejects, and
/// the pnpm a project pins has to be reachable from the one that is running,
/// so the pass that switches to it reads what it can.
///
/// Fails with `read`'s error for the complete document when it is not a
/// block mapping with one key per line, when no setting is rejected on its
/// own, or when the settings left are still rejected together.
pub(crate) fn read_readable_settings<Error>(
    text: &str,
    read: impl Fn(&str) -> Result<WorkspaceSettings, Error>,
) -> Result<WorkspaceSettings, Error> {
    let error = match read(text) {
        Ok(settings) => return Ok(settings),
        Err(error) => error,
    };
    let Some(key_lines) = top_level_key_lines(text) else {
        return Err(error);
    };
    let mut lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut left_out_any = false;
    for setting in setting_lines(&key_lines, lines.len()) {
        if read(&lines[setting.clone()].concat()).is_err() {
            // Blanked rather than removed, so what is left keeps its lines.
            lines[setting].fill("\n");
            left_out_any = true;
        }
    }
    if !left_out_any {
        return Err(error);
    }
    read(&lines.concat()).or(Err(error))
}

/// The [`PIN_DECISION_SETTINGS`] of a document that is not a block mapping
/// with one key per line, such as a flow mapping, written one per line so
/// [`read_readable_settings`] can leave each out on its own. JSON is YAML, so
/// each key and value is written as JSON. A document that is not a mapping
/// keeps none of its settings.
fn rewrite_selected_settings(text: &str) -> String {
    let Ok(settings) = serde_saphyr::from_str::<IndexMap<String, serde_json::Value>>(text) else {
        return String::new();
    };
    let mut selected = String::new();
    for (key, value) in settings
        .into_iter()
        .filter(|(key, _)| PIN_DECISION_SETTINGS.contains(&key.as_str()))
    {
        let _ = writeln!(selected, "{}: {value}", serde_json::Value::String(key));
    }
    selected
}

/// The 0-based line range of each top-level setting, given the line of each
/// top-level key and the document's `line_count`.
fn setting_lines(key_lines: &[usize], line_count: usize) -> impl Iterator<Item = Range<usize>> {
    key_lines
        .iter()
        .enumerate()
        .map(move |(index, &start)| {
            let end = key_lines
                .get(index + 1)
                .copied()
                .unwrap_or(line_count);
            start..end.min(line_count)
        })
}

/// The 0-based line of each top-level key, in document order, or `None`
/// when the document is not a block mapping with one key per line.
fn top_level_key_lines(text: &str) -> Option<Vec<usize>> {
    top_level_keys::<IgnoredAny>(text)
        .map(|keys| {
            keys.into_iter()
                .map(|(line, _)| line)
                .collect()
        })
}

/// Each top-level key with its 0-based line, in document order, or `None`
/// when the document is not a block mapping with one key per line.
fn top_level_keys<Key: for<'de> Deserialize<'de>>(text: &str) -> Option<Vec<(usize, Key)>> {
    let TopLevelKeys(keys) = serde_saphyr::from_str::<TopLevelKeys<Key>>(text).ok()?;
    let mut lines: Vec<(usize, Key)> = Vec::with_capacity(keys.len());
    for key in keys {
        let line = usize::try_from(key.referenced.line()).ok()?.checked_sub(1)?;
        if key.referenced.column() != 1
            || lines
                .last()
                .is_some_and(|&(last, _)| last >= line)
        {
            return None;
        }
        lines.push((line, key.value));
    }
    Some(lines)
}

struct TopLevelKeys<Key>(Vec<Spanned<Key>>);

impl<'de, Key: Deserialize<'de>> Deserialize<'de> for TopLevelKeys<Key> {
    fn deserialize<De: Deserializer<'de>>(deserializer: De) -> Result<Self, De::Error> {
        deserializer.deserialize_map(TopLevelKeysVisitor(PhantomData))
    }
}

struct TopLevelKeysVisitor<Key>(PhantomData<Key>);

impl<'de, Key: Deserialize<'de>> Visitor<'de> for TopLevelKeysVisitor<Key> {
    type Value = TopLevelKeys<Key>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a mapping")
    }

    fn visit_map<Map: MapAccess<'de>>(self, mut map: Map) -> Result<Self::Value, Map::Error> {
        let mut keys = Vec::new();
        while let Some(key) = map.next_key::<Spanned<Key>>()? {
            map.next_value::<IgnoredAny>()?;
            keys.push(key);
        }
        Ok(TopLevelKeys(keys))
    }
}
