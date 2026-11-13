#[cfg(test)]
mod tests;
mod unescape;

use self::unescape::{
    unescape_yaml_double_quoted_lenient, unescape_yaml_double_quoted_strict,
    unescape_yaml_single_quoted_strict,
};
use crate::placeholder_ranges;
use std::{fmt::Write as _, ops::Range};

const SENTINEL_STEM: &str = "pnpmplaceholder";
const MIN_SENTINEL_PREFIX_LEN: usize = SENTINEL_STEM.len() + 2;
const RAW_STYLE_PROBE: &str = r"\/''";
const PLAIN_STYLE_MARKER: &str = r"\/''";
const SINGLE_QUOTE_STYLE_MARKER: &str = r"\/'";
const DOUBLE_QUOTE_STYLE_MARKER: &str = "/''";

#[derive(Clone, Copy)]
enum ScalarQuoteStyle {
    Plain,
    Single,
    Double,
}

/// Single-line unescaped `${...}` placeholders in a YAML document, paired with
/// a collision-free sentinel prefix (`pnpmplaceholder<N>_`) absent from the
/// source text (raw and after YAML double-quoted unescaping) and any extra
/// strings.
///
/// Each placeholder's sentinel (`{prefix}\/''{index}_`) carries a scalar-style
/// probe (`\/''`) that YAML leaves untouched in plain/block scalars (`\/''`),
/// folds `''` to `'` in single-quoted scalars (`\/'`), and unescapes `\/` to
/// `/` in double-quoted scalars (`/''`), allowing [`Self::try_restore`] to
/// apply the exact YAML quote-unescaping rules of the enclosing scalar in one
/// pass without iterative re-parsing.
pub struct SentinelPlaceholders {
    prefix: String,
    ranges: Vec<Range<usize>>,
}

impl SentinelPlaceholders {
    /// Byte ranges of unescaped `${...}` placeholders in `text` that do not
    /// span a line break.
    #[must_use]
    pub fn single_line_ranges(text: &str) -> Vec<Range<usize>> {
        placeholder_ranges(text)
            .into_iter()
            .filter(|range| !text[range.clone()].contains(['\r', '\n']))
            .collect()
    }

    /// Build sentinel metadata for `text`, returning `None` when `text` has no
    /// single-line unescaped `${...}` placeholder.
    #[must_use]
    pub fn new(text: &str) -> Option<Self> {
        Self::with_extra(text, Self::single_line_ranges(text), std::iter::empty())
    }

    /// Build sentinel metadata for `ranges` in `text`, choosing a prefix that
    /// also avoids colliding with every string in `extra`.
    #[must_use]
    pub fn with_extra<'extra>(
        text: &str,
        ranges: Vec<Range<usize>>,
        extra: impl IntoIterator<Item = &'extra str>,
    ) -> Option<Self> {
        if ranges.is_empty() {
            return None;
        }
        let extra: Vec<&str> = extra.into_iter().collect();
        let prefix = unique_sentinel_prefix(text, &extra);
        Some(Self { prefix, ranges })
    }

    #[must_use]
    pub fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }

    #[must_use]
    pub fn sentinel(&self, index: usize) -> String {
        format!("{}{RAW_STYLE_PROBE}{index}_", self.prefix)
    }

    /// Render `text` replacing every placeholder with its style-probing
    /// sentinel.
    #[must_use]
    pub fn replace_all(&self, text: &str) -> String {
        let mut rendered = String::with_capacity(text.len());
        let mut copied = 0;
        for (index, range) in self.ranges.iter().enumerate() {
            rendered.push_str(&text[copied..range.start]);
            let _ = write!(rendered, "{}{RAW_STYLE_PROBE}{index}_", self.prefix);
            copied = range.end;
        }
        rendered.push_str(&text[copied..]);
        rendered
    }

    #[must_use]
    pub fn may_contain_sentinel(&self, value: &str) -> bool {
        value.contains(&self.prefix)
    }

    /// Restore every sentinel in `value` back to its `${...}` text decoded
    /// according to the YAML scalar style that parsed it, returning `None` if
    /// a quoted placeholder contains an unescaped quote or invalid escape.
    #[must_use]
    pub fn try_restore(&self, original_text: &str, value: &str) -> Option<String> {
        let mut restored = String::with_capacity(value.len());
        let mut cursor = 0;
        while let Some(offset) = value[cursor..].find(&self.prefix) {
            let start = cursor + offset;
            restored.push_str(&value[cursor..start]);
            let (style, index, end) = parse_sentinel_at(value, start + self.prefix.len())?;
            let raw = &original_text[self.ranges.get(index)?.clone()];
            match style {
                ScalarQuoteStyle::Plain => restored.push_str(raw),
                ScalarQuoteStyle::Single => {
                    restored.push_str(&unescape_yaml_single_quoted_strict(raw)?);
                }
                ScalarQuoteStyle::Double => {
                    restored.push_str(&unescape_yaml_double_quoted_strict(raw)?);
                }
            }
            cursor = end;
        }
        restored.push_str(&value[cursor..]);
        Some(restored)
    }

    pub fn try_restore_in_place(&self, original_text: &str, value: &mut String) -> bool {
        if !self.may_contain_sentinel(value) {
            return true;
        }
        let Some(restored) = self.try_restore(original_text, value) else {
            return false;
        };
        *value = restored;
        true
    }
}

fn parse_sentinel_at(value: &str, after_prefix: usize) -> Option<(ScalarQuoteStyle, usize, usize)> {
    let rest = &value[after_prefix..];
    let (style, digits_rest, marker_len) = if let Some(tail) = rest.strip_prefix(PLAIN_STYLE_MARKER)
    {
        (ScalarQuoteStyle::Plain, tail, PLAIN_STYLE_MARKER.len())
    } else if let Some(tail) = rest.strip_prefix(SINGLE_QUOTE_STYLE_MARKER) {
        (ScalarQuoteStyle::Single, tail, SINGLE_QUOTE_STYLE_MARKER.len())
    } else {
        let tail = rest.strip_prefix(DOUBLE_QUOTE_STYLE_MARKER)?;
        (ScalarQuoteStyle::Double, tail, DOUBLE_QUOTE_STYLE_MARKER.len())
    };
    let digits_len = digits_rest.find('_')?;
    let index: usize = digits_rest[..digits_len].parse().ok()?;
    Some((style, index, after_prefix + marker_len + digits_len + 1))
}

fn unique_sentinel_prefix(text: &str, extra: &[&str]) -> String {
    let unescaped = text.contains('\\').then(|| unescape_yaml_double_quoted_lenient(text));
    let extra_bytes: usize = extra
        .iter()
        .map(|item| item.len())
        .sum();
    let total_bytes = text.len() + unescaped.as_ref().map_or(0, String::len) + extra_bytes;
    let mut occupied = vec![false; total_bytes / MIN_SENTINEL_PREFIX_LEN + 1];
    mark_occupied_prefixes(text, &mut occupied);
    if let Some(unescaped) = &unescaped {
        mark_occupied_prefixes(unescaped, &mut occupied);
    }
    for value in extra {
        mark_occupied_prefixes(value, &mut occupied);
    }
    let counter = occupied
        .iter()
        .position(|&used| !used)
        .unwrap_or(occupied.len());
    format!("{SENTINEL_STEM}{counter}_")
}

fn mark_occupied_prefixes(haystack: &str, occupied: &mut [bool]) {
    let mut cursor = 0;
    while let Some(offset) = haystack[cursor..].find(SENTINEL_STEM) {
        let after_stem = cursor + offset + SENTINEL_STEM.len();
        let rest = &haystack.as_bytes()[after_stem..];
        let digits_len = rest
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits_len > 0 && rest.get(digits_len) == Some(&b'_') {
            if let Ok(index) = haystack[after_stem..after_stem + digits_len].parse::<usize>()
                && let Some(slot) = occupied.get_mut(index)
            {
                *slot = true;
            }
            cursor = after_stem + digits_len + 1;
        } else {
            cursor = after_stem;
        }
    }
}
