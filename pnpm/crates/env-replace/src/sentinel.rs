use crate::placeholder_ranges;
use std::{fmt::Write as _, ops::Range};

const SENTINEL_STEM: &str = "pnpmplaceholder";
const MIN_SENTINEL_PREFIX_LEN: usize = SENTINEL_STEM.len() + 2;
const MAX_DEMOTE_READS: u32 = 512;

/// Single-line unescaped `${...}` placeholders in a YAML document, paired with
/// a bare-token sentinel prefix (`pnpmplaceholder<N>_`) absent from the source
/// text (raw and after YAML double-quoted unescaping) and any extra strings.
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
        format!("{}{index}_", self.prefix)
    }

    /// Render `text` replacing every placeholder with its `{prefix}{index}_`
    /// sentinel.
    #[must_use]
    pub fn replace_all(&self, text: &str) -> String {
        let mut rendered = String::with_capacity(text.len());
        let mut copied = 0;
        for (index, range) in self.ranges.iter().enumerate() {
            rendered.push_str(&text[copied..range.start]);
            let _ = write!(rendered, "{}{index}_", self.prefix);
            copied = range.end;
        }
        rendered.push_str(&text[copied..]);
        rendered
    }

    /// Render `text` replacing only the placeholders `accepts` cannot parse
    /// without, returning `None` when `accepts` rejects full sentinel
    /// replacement or when probing exceeds the read budget.
    pub fn replace_minimal(
        &self,
        text: &str,
        mut accepts: impl FnMut(&str) -> bool,
    ) -> Option<String> {
        let mut use_sentinel = vec![true; self.ranges.len()];
        if !accepts(&self.render_subset(text, &use_sentinel)) {
            return None;
        }
        let indices: Vec<usize> = (0..self.ranges.len()).collect();
        let mut reads = 1u32;
        if !self.demote_group(text, &indices, &mut use_sentinel, &mut reads, &mut accepts) {
            return None;
        }
        Some(self.render_subset(text, &use_sentinel))
    }

    #[must_use]
    pub fn may_contain_sentinel(&self, value: &str) -> bool {
        value.contains(&self.prefix)
    }

    pub fn restore_in_place(&self, original_text: &str, value: &mut String) {
        if self.may_contain_sentinel(value) {
            *value = self.restore(original_text, value);
        }
    }

    /// Replace every `{prefix}{index}_` token in `value` with the original
    /// `&original_text[range]` slice.
    #[must_use]
    pub fn restore(&self, original_text: &str, value: &str) -> String {
        let mut restored = String::with_capacity(value.len());
        let mut cursor = 0;
        while let Some(offset) = value[cursor..].find(&self.prefix) {
            let start = cursor + offset;
            restored.push_str(&value[cursor..start]);
            let after_prefix = start + self.prefix.len();
            if let Some((raw_placeholder, end)) =
                self.parse_sentinel_at(original_text, value, after_prefix)
            {
                restored.push_str(raw_placeholder);
                cursor = end;
            } else {
                restored.push_str(&self.prefix);
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
        let range = self.ranges.get(index)?;
        Some((&original_text[range.clone()], after_prefix + digits_len + 1))
    }

    fn render_subset(&self, text: &str, use_sentinel: &[bool]) -> String {
        let mut rendered = String::with_capacity(text.len());
        let mut copied = 0;
        for (index, range) in self.ranges.iter().enumerate() {
            rendered.push_str(&text[copied..range.start]);
            if use_sentinel[index] {
                let _ = write!(rendered, "{}{index}_", self.prefix);
            } else {
                rendered.push_str(&text[range.clone()]);
            }
            copied = range.end;
        }
        rendered.push_str(&text[copied..]);
        rendered
    }

    fn demote_group(
        &self,
        text: &str,
        group: &[usize],
        use_sentinel: &mut [bool],
        reads: &mut u32,
        accepts: &mut impl FnMut(&str) -> bool,
    ) -> bool {
        if group.is_empty() {
            return true;
        }
        if *reads >= MAX_DEMOTE_READS {
            return false;
        }
        *reads += 1;
        for &index in group {
            use_sentinel[index] = false;
        }
        if accepts(&self.render_subset(text, use_sentinel)) {
            return true;
        }
        for &index in group {
            use_sentinel[index] = true;
        }
        if group.len() == 1 {
            return true;
        }
        let (left, right) = group.split_at(group.len() / 2);
        self.demote_group(text, left, use_sentinel, reads, accepts)
            && self.demote_group(text, right, use_sentinel, reads, accepts)
    }
}

fn unique_sentinel_prefix(text: &str, extra: &[&str]) -> String {
    let unescaped = text.contains('\\').then(|| unescape_yaml_double_quoted(text));
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
    for item in extra {
        mark_occupied_prefixes(item, &mut occupied);
    }
    let counter = occupied
        .iter()
        .position(|&used| !used)
        .unwrap_or(0);
    format!("{SENTINEL_STEM}{counter}_")
}

fn mark_occupied_prefixes(haystack: &str, occupied: &mut [bool]) {
    for (offset, _) in haystack.match_indices(SENTINEL_STEM) {
        let after = &haystack[offset + SENTINEL_STEM.len()..];
        let digits_len = after
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        if digits_len > 0
            && after.as_bytes().get(digits_len) == Some(&b'_')
            && let Ok(counter) = after[..digits_len].parse::<usize>()
            && let Some(slot) = occupied.get_mut(counter)
        {
            *slot = true;
        }
    }
}

fn unescape_yaml_double_quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            let Some(character) = text[index..].chars().next() else {
                break;
            };
            out.push(character);
            index += character.len_utf8();
            continue;
        }
        match bytes.get(index + 1).copied() {
            Some(b'\\') => {
                out.push('\\');
                index += 2;
            }
            Some(b'x') => index += push_hex_escape(&text[index + 2..], 2, &mut out),
            Some(b'u') => index += push_hex_escape(&text[index + 2..], 4, &mut out),
            Some(b'U') => index += push_hex_escape(&text[index + 2..], 8, &mut out),
            Some(b' ' | b'\t' | b'\r' | b'\n') => {
                if let Some(consumed) = escaped_line_break_len(&bytes[index + 1..]) {
                    index += 1 + consumed;
                } else {
                    out.push(' ');
                    index += 1;
                }
            }
            _ => {
                out.push(' ');
                index += 1;
            }
        }
    }
    out
}

fn push_hex_escape(rest: &str, hex_digits: usize, out: &mut String) -> usize {
    if rest
        .as_bytes()
        .get(..hex_digits)
        .is_some_and(|slice| slice.iter().all(u8::is_ascii_hexdigit))
        && let Ok(codepoint) = u32::from_str_radix(&rest[..hex_digits], 16)
        && let Some(decoded) = char::from_u32(codepoint)
    {
        out.push(decoded);
        2 + hex_digits
    } else {
        out.push(' ');
        1
    }
}

fn escaped_line_break_len(after_backslash: &[u8]) -> Option<usize> {
    let trailing_spaces = after_backslash
        .iter()
        .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
        .count();
    let newline_len = match after_backslash.get(trailing_spaces..) {
        Some([b'\r', b'\n', ..]) => 2,
        Some([b'\r' | b'\n', ..]) => 1,
        _ => return None,
    };
    let leading_indent = after_backslash[trailing_spaces + newline_len..]
        .iter()
        .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
        .count();
    Some(trailing_spaces + newline_len + leading_indent)
}
