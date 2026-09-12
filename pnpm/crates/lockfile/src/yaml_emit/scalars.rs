use super::{INDENT, resolves_implicitly};

/// Scalar styles, mirroring the fork's `STYLE_*` constants. `Folded` never
/// occurs here because `lineWidth` is `-1`.
enum ScalarStyle {
    Plain,
    Single,
    Double,
    Literal,
}

/// Render a string scalar. Mirrors the fork's `writeScalar` under the lockfile
/// options (`quotingType` single, `noCompatMode`, `lineWidth: -1`,
/// `forceQuotes` off).
pub(super) fn write_scalar(string: &str, level: usize, single_line: bool, inblock: bool) -> String {
    if string.is_empty() {
        return "''".to_string();
    }
    match choose_scalar_style(string, single_line, inblock) {
        ScalarStyle::Plain => string.to_string(),
        ScalarStyle::Single => format!("'{}'", string.replace('\'', "''")),
        ScalarStyle::Double => format!(r#""{}""#, escape_string(string)),
        ScalarStyle::Literal => {
            let indent = INDENT * level.max(1);
            format!(
                "|{}{}",
                block_header(string),
                drop_ending_newline(&indent_string(string, indent)),
            )
        }
    }
}

/// Mirrors the fork's `chooseScalarStyle` under lockfile options.
fn choose_scalar_style(string: &str, single_line_only: bool, inblock: bool) -> ScalarStyle {
    let chars: Vec<u32> = string.chars().map(u32::from).collect();
    let Some(scan) = scan_scalar(&chars, single_line_only, inblock) else {
        return ScalarStyle::Double;
    };
    if scan.has_line_break {
        return ScalarStyle::Literal;
    }
    if scan.plain && !resolves_implicitly(string) {
        return ScalarStyle::Plain;
    }
    ScalarStyle::Single
}

/// What the scan of a scalar's characters found, or `None` when one of them
/// is unprintable and the scalar has to be double-quoted.
struct ScalarScan {
    plain: bool,
    has_line_break: bool,
}

/// A `single_line_only` scalar has nowhere to put a line break, so a line
/// feed counts as unprintable rather than selecting the literal style.
fn scan_scalar(chars: &[u32], single_line_only: bool, inblock: bool) -> Option<ScalarScan> {
    let mut scan = ScalarScan {
        plain: is_plain_safe_first(chars[0]) && is_plain_safe_last(chars[chars.len() - 1]),
        has_line_break: false,
    };
    let mut prev: Option<u32> = None;
    for &char in chars {
        if char == CHAR_LINE_FEED && !single_line_only {
            scan.has_line_break = true;
        } else if !is_printable(char) {
            return None;
        }
        scan.plain = scan.plain && is_plain_safe(char, prev, inblock);
        prev = Some(char);
    }
    Some(scan)
}

const CHAR_TAB: u32 = 0x09;

const CHAR_LINE_FEED: u32 = 0x0A;

const CHAR_CARRIAGE_RETURN: u32 = 0x0D;

const CHAR_SPACE: u32 = 0x20;

const CHAR_SHARP: u32 = 0x23;

// #
const CHAR_COLON: u32 = 0x3A;

// :
const CHAR_COMMA: u32 = 0x2C;

// ,
const CHAR_LEFT_SQUARE_BRACKET: u32 = 0x5B;

// [
const CHAR_RIGHT_SQUARE_BRACKET: u32 = 0x5D;

// ]
const CHAR_LEFT_CURLY_BRACKET: u32 = 0x7B;

// {
const CHAR_RIGHT_CURLY_BRACKET: u32 = 0x7D;

// }
const CHAR_BOM: u32 = 0xFEFF;

fn is_whitespace(code: u32) -> bool {
    code == CHAR_SPACE || code == CHAR_TAB
}

fn is_printable(code: u32) -> bool {
    (0x00020..=0x00007E).contains(&code)
        || ((0x000A1..=0x00D7FF).contains(&code) && code != 0x2028 && code != 0x2029)
        || ((0x0E000..=0x00FFFD).contains(&code) && code != CHAR_BOM)
        || (0x10000..=0x10FFFF).contains(&code)
}

fn is_ns_char_or_whitespace(code: u32) -> bool {
    is_printable(code) && code != CHAR_BOM && code != CHAR_CARRIAGE_RETURN && code != CHAR_LINE_FEED
}

fn is_plain_safe_first(code: u32) -> bool {
    is_printable(code)
        && code != CHAR_BOM
        && !is_whitespace(code)
        && !matches!(
            code,
            0x2D | // -
            0x3F | // ?
            CHAR_COLON
                | CHAR_COMMA
                | CHAR_LEFT_SQUARE_BRACKET
                | CHAR_RIGHT_SQUARE_BRACKET
                | CHAR_LEFT_CURLY_BRACKET
                | CHAR_RIGHT_CURLY_BRACKET
                | CHAR_SHARP
                | 0x26 | // &
            0x2A | // *
            0x21 | // !
            0x7C | // |
            0x3D | // =
            0x3E | // >
            0x27 | // '
            0x22 | // "
            0x25 | // %
            0x40 | // @
            0x60, // `
        )
}

fn is_plain_safe_last(code: u32) -> bool {
    !is_whitespace(code) && code != CHAR_COLON
}

fn is_plain_safe(code: u32, prev: Option<u32>, inblock: bool) -> bool {
    let code_is_ns_or_ws = is_ns_char_or_whitespace(code);
    let code_is_ns = code_is_ns_or_ws && !is_whitespace(code);
    let base = code_is_ns_or_ws && (inblock || !is_flow_indicator(code));
    let prev_is_colon = prev == Some(CHAR_COLON);
    let prev_is_ns =
        prev.is_some_and(|prev| is_ns_char_or_whitespace(prev) && !is_whitespace(prev));
    // change to true on '[^ ]#'
    if prev_is_ns && code == CHAR_SHARP {
        return true;
    }
    // change to true on ':[^ ]'
    if prev_is_colon && code_is_ns {
        return true;
    }
    // ns-plain-char: a non-`#` base character that isn't the `: ` sequence.
    base && code != CHAR_SHARP && (!prev_is_colon || code_is_ns)
}

/// The characters that end a plain scalar inside a flow collection.
fn is_flow_indicator(code: u32) -> bool {
    matches!(
        code,
        CHAR_COMMA
            | CHAR_LEFT_SQUARE_BRACKET
            | CHAR_RIGHT_SQUARE_BRACKET
            | CHAR_LEFT_CURLY_BRACKET
            | CHAR_RIGHT_CURLY_BRACKET,
    )
}

/// Mirrors the fork's `escapeString` (with `escapeSeq` table and hex fallback).
fn escape_string(string: &str) -> String {
    let mut result = String::new();
    for ch in string.chars() {
        let code = u32::from(ch);
        if let Some(seq) = escape_sequence(code) {
            result.push_str(seq);
        } else if is_printable(code) {
            result.push(ch);
        } else {
            result.push_str(&encode_hex(code));
        }
    }
    result
}

fn escape_sequence(code: u32) -> Option<&'static str> {
    Some(match code {
        0x00 => r"\0",
        0x07 => r"\a",
        0x08 => r"\b",
        0x09 => r"\t",
        0x0A => r"\n",
        0x0B => r"\v",
        0x0C => r"\f",
        0x0D => r"\r",
        0x1B => r"\e",
        0x22 => r#"\""#,
        0x5C => r"\\",
        0x85 => r"\N",
        0xA0 => r"\_",
        0x2028 => r"\L",
        0x2029 => r"\P",
        _ => return None,
    })
}

fn encode_hex(code: u32) -> String {
    let hex = format!("{code:X}");
    let (handle, width) = if code <= 0xFF {
        ('x', 2)
    } else if code <= 0xFFFF {
        ('u', 4)
    } else {
        ('U', 8)
    };
    format!(r"\{handle}{hex:0>width$}")
}

fn block_header(string: &str) -> String {
    let indicator = if needs_indent_indicator(string) { INDENT.to_string() } else { String::new() };
    let bytes = string.as_bytes();
    let clip = bytes.last() == Some(&b'\n');
    let keep = clip && (bytes.get(bytes.len().wrapping_sub(2)) == Some(&b'\n') || string == "\n");
    let chomp = if keep {
        "+"
    } else if clip {
        ""
    } else {
        "-"
    };
    format!("{indicator}{chomp}\n")
}

fn needs_indent_indicator(string: &str) -> bool {
    let trimmed = string.trim_start_matches('\n');
    trimmed.starts_with(' ')
}

fn drop_ending_newline(string: &str) -> String {
    string.strip_suffix('\n').unwrap_or(string).to_string()
}

fn indent_string(string: &str, spaces: usize) -> String {
    let indent: String = std::iter::repeat_n(' ', spaces).collect();
    let mut result = String::new();
    for line in split_keep_newlines(string) {
        if !line.is_empty() && line != "\n" {
            result.push_str(&indent);
        }
        result.push_str(line);
    }
    result
}

/// Split into lines that keep their trailing `\n`, mirroring the manual scan in
/// the fork's `indentString`.
fn split_keep_newlines(string: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = string.as_bytes();
    while start < bytes.len() {
        if let Some(offset) = string[start..].find('\n') {
            lines.push(&string[start..=(start + offset)]);
            start += offset + 1;
        } else {
            lines.push(&string[start..]);
            start = bytes.len();
        }
    }
    lines
}
