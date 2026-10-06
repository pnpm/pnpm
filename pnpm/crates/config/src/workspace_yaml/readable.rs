use super::{Deserialize, Deserializer, IgnoredAny, WorkspaceSettings};
use serde::de::{MapAccess, Visitor};
use serde_saphyr::Spanned;
use std::{fmt, ops::Range};

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
    let TopLevelKeys(keys) = serde_saphyr::from_str(text).ok()?;
    let mut lines = Vec::with_capacity(keys.len());
    for key in keys {
        let line = usize::try_from(key.referenced.line()).ok()?.checked_sub(1)?;
        if key.referenced.column() != 1
            || lines
                .last()
                .is_some_and(|&last| last >= line)
        {
            return None;
        }
        lines.push(line);
    }
    Some(lines)
}

struct TopLevelKeys(Vec<Spanned<IgnoredAny>>);

impl<'de> Deserialize<'de> for TopLevelKeys {
    fn deserialize<De: Deserializer<'de>>(deserializer: De) -> Result<Self, De::Error> {
        deserializer.deserialize_map(TopLevelKeysVisitor)
    }
}

struct TopLevelKeysVisitor;

impl<'de> Visitor<'de> for TopLevelKeysVisitor {
    type Value = TopLevelKeys;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a mapping")
    }

    fn visit_map<Map: MapAccess<'de>>(self, mut map: Map) -> Result<Self::Value, Map::Error> {
        let mut keys = Vec::new();
        while let Some(key) = map.next_key::<Spanned<IgnoredAny>>()? {
            map.next_value::<IgnoredAny>()?;
            keys.push(key);
        }
        Ok(TopLevelKeys(keys))
    }
}
