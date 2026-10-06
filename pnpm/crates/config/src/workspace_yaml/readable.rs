use super::{Deserialize, Deserializer, EnvVar, IgnoredAny, WorkspaceSettings, parse_settings};
use serde::de::{MapAccess, Visitor};
use serde_saphyr::Spanned;
use std::fmt;

/// How the settings of a `pnpm-workspace.yaml` are read from its text.
pub(crate) type ReadSettings = fn(&str) -> Result<WorkspaceSettings, Box<serde_saphyr::Error>>;

/// [`parse_settings`], or [`parse_readable_settings`] when `skip_unreadable`.
pub(crate) fn settings_reader<Sys: EnvVar>(skip_unreadable: bool) -> ReadSettings {
    if skip_unreadable { parse_readable_settings::<Sys> } else { parse_settings::<Sys> }
}

/// [`parse_settings`], leaving out each top-level setting that does not
/// parse.
///
/// A newer pnpm can give a setting a shape this one rejects, and the pnpm a
/// project pins has to be reachable from the one that is running, so the
/// pass that switches to it reads what it can.
///
/// Fails with the error of the complete document when it is not a block
/// mapping, or when an error does not point into a top-level setting.
pub(crate) fn parse_readable_settings<Sys: EnvVar>(
    text: &str,
) -> Result<WorkspaceSettings, Box<serde_saphyr::Error>> {
    let first_error = match parse_settings::<Sys>(text) {
        Ok(settings) => return Ok(settings),
        Err(error) => error,
    };
    let Some(key_lines) = top_level_key_lines(text) else {
        return Err(first_error);
    };
    let mut lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut failed_at = first_error.location();
    for _ in 0..key_lines.len() {
        let Some(setting) =
            failed_at.and_then(|at| setting_lines(&key_lines, lines.len(), at.line()))
        else {
            break;
        };
        // Blanked rather than removed, so later errors keep their line.
        lines[setting].fill("\n");
        match parse_settings::<Sys>(&lines.concat()) {
            Ok(settings) => return Ok(settings),
            Err(error) => failed_at = error.location(),
        }
    }
    Err(first_error)
}

/// The 0-based line range, within `line_count`, of the top-level setting that
/// `line` (1-based) is in.
fn setting_lines(
    key_lines: &[usize],
    line_count: usize,
    line: u64,
) -> Option<std::ops::Range<usize>> {
    let line = usize::try_from(line).ok()?.checked_sub(1)?;
    let index = key_lines
        .partition_point(|&start| start <= line)
        .checked_sub(1)?;
    let end = key_lines
        .get(index + 1)
        .copied()
        .unwrap_or(line_count)
        .min(line_count);
    let start = key_lines[index];
    (start < end).then_some(start..end)
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
