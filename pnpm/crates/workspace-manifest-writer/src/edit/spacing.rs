use super::{Line, lines, top_level_key_line};

/// Whether the blank lines that end at `line_start` are the tail of a
/// keep-chomped block scalar rather than a separator. Such a scalar keeps
/// the blank lines that follow it *as its value*, so dropping them would
/// rewrite a setting the caller never asked to touch.
///
/// The blanks belong to the scalar when the content above them climbs out to
/// a keep-chomping header: walking up, each line that is less indented than
/// everything seen so far either is that header or becomes the new bar to
/// clear. A top-level line that is not a header ends the search — a scalar's
/// body is always indented past its own key.
pub(super) fn blanks_belong_to_kept_scalar(text: &str, line_start: usize) -> bool {
    let all = lines(&text[..line_start]);
    let mut enclosing_indent = usize::MAX;
    for line in all.iter().rev().filter(|line| !line.content.trim().is_empty()) {
        let indent = indent_width(line.content);
        if indent >= enclosing_indent {
            continue;
        }
        if is_kept_chomping_header(line.content) {
            return true;
        }
        if indent == 0 {
            return false;
        }
        enclosing_indent = indent;
    }
    false
}

/// Whether `content` declares a block scalar that keeps its trailing line
/// breaks. Only the value position counts — a `|+` inside an ordinary
/// scalar, inside a comment, or inside a quoted key is text.
fn is_kept_chomping_header(content: &str) -> bool {
    let mut line = content.trim_start();
    while let Some(item) = line.strip_prefix("- ") {
        line = item.trim_start();
    }
    opens_kept_chomping_scalar(line) || line_value(line).is_some_and(opens_kept_chomping_scalar)
}

/// The value of a `key: value` line, or `None` when the line declares none.
/// The delimiter is the first `:` that ends the line or is followed by
/// whitespace *outside* a quoted scalar and before any comment, so neither a
/// quoted key holding `: ` nor a comment holding one is mistaken for it.
fn line_value(line: &str) -> Option<&str> {
    let mut quote = None;
    let mut escaped = false;
    for (index, char) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if let Some(open) = quote {
            escaped = escapes_next(line, index, char, open);
            quote = (escaped || char != open).then_some(open);
        } else {
            match scan_outside_scalar(line, index, char) {
                Scan::OpensScalar => quote = Some(char),
                Scan::Comment => return None,
                Scan::Value(value) => return Some(value),
                Scan::Keep => {}
            }
        }
    }
    None
}

/// Whether the character escapes the next one inside an open scalar: a
/// backslash in a double-quoted scalar, or the first of a doubled quote in a
/// single-quoted one.
fn escapes_next(line: &str, index: usize, char: char, open: char) -> bool {
    match open {
        '"' => char == '\\',
        _ => char == '\'' && line[index + 1..].starts_with('\''),
    }
}

/// What one character means outside a quoted scalar.
enum Scan<'a> {
    OpensScalar,
    /// The rest of the line is a comment, so the line declares no value.
    Comment,
    /// The `key:` delimiter, with the value that follows it.
    Value(&'a str),
    /// Part of the key.
    Keep,
}

fn scan_outside_scalar(line: &str, index: usize, char: char) -> Scan<'_> {
    match char {
        // A quote only opens a scalar at the start of a token: the
        // apostrophe in a plain key like `it's` is part of the key.
        '\'' | '"'
            if index == 0 || line[..index].ends_with([' ', '\t', ':', '-', '[', '{', ',']) =>
        {
            Scan::OpensScalar
        }
        '#' if index == 0 || line[..index].ends_with([' ', '\t']) => Scan::Comment,
        ':' => {
            let value = &line[index + 1..];
            if value.is_empty() || value.starts_with([' ', '\t']) {
                return Scan::Value(value.trim_start());
            }
            Scan::Keep
        }
        _ => Scan::Keep,
    }
}

/// Whether `value` is a block scalar header carrying a `+`, in either order
/// relative to an explicit indentation digit (`|+`, `>+2`, `|2+`), with or
/// without a trailing comment. Any anchor and tag properties in front of the
/// header (`&notes |+`, `!!str >+`) are skipped.
fn opens_kept_chomping_scalar(value: &str) -> bool {
    let mut value = value.trim_start();
    while value.starts_with(['&', '!']) {
        let Some((_, rest)) = value.split_once([' ', '\t']) else {
            return false;
        };
        value = rest.trim_start();
    }
    let Some(indicators) = value.strip_prefix(['|', '>']) else {
        return false;
    };
    let indicators = indicators.split_whitespace().next().unwrap_or_default();
    indicators.contains('+') && indicators.chars().all(|char| char == '+' || char.is_ascii_digit())
}

/// Leading-space count of `content`, whatever the line holds — unlike
/// [`structural_indent`](crate::edit::scanning::structural_indent), which reads a comment or a blank as unindented.
/// Block scalar bodies can hold both.
fn indent_width(content: &str) -> usize {
    content.len() - content.trim_start().len()
}

/// Start of the run of blank lines immediately preceding `line_start`, or
/// `line_start` itself when the preceding line is not blank.
pub(super) fn blank_run_start(text: &str, line_start: usize) -> usize {
    let mut start = line_start;
    for line in lines(&text[..line_start]).iter().rev() {
        if !line.content.trim().is_empty() {
            break;
        }
        start = line.start;
    }
    start
}

/// Whether every original non-first top-level key has a blank line before it.
/// Judged once, on the document as parsed: an edit that drops a key must not
/// change how the surviving blocks are separated.
pub(crate) fn uses_blank_line_style(text: &str, top_level_keys: &[String]) -> bool {
    if top_level_keys.len() < 2 {
        return false;
    }
    let all = lines(text);
    let mut non_first = 0;
    let mut non_first_with_blank = 0;
    for key in &top_level_keys[1..] {
        let Some(idx) = top_level_key_line(&all, key) else {
            continue;
        };
        non_first += 1;
        if has_blank_before(&all, idx) {
            non_first_with_blank += 1;
        }
    }
    non_first > 0 && non_first == non_first_with_blank
}

/// Whether `text`'s final line is blank, by the same trimmed-content test
/// [`blank_run_start`] and [`has_blank_before`] use — so a whitespace-only
/// separator counts as one too.
pub(super) fn ends_with_blank_line(text: &str) -> bool {
    lines(text).last().is_some_and(|line| line.content.trim().is_empty())
}

/// Whether a blank line precedes the key at `idx`, looking past the key's own
/// leading comment lines.
fn has_blank_before(all: &[Line<'_>], idx: usize) -> bool {
    let mut cursor = idx;
    while cursor > 0 {
        let prev = &all[cursor - 1];
        let trimmed = prev.content.trim_start();
        if trimmed.is_empty() {
            return true;
        }
        if trimmed.starts_with('#') {
            cursor -= 1;
            continue;
        }
        return false;
    }
    false
}
