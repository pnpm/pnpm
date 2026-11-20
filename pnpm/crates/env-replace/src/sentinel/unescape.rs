//! YAML 1.2 quote-escape decoders for single-line `${...}` placeholder slices.
//!
//! `pnpm-env-replace` stays dependency-free at the base crate layer, and
//! wrapping a raw placeholder slice in quotes for a full YAML parser would let
//! an unescaped quote followed by ` #` truncate the scalar into a YAML comment
//! rather than rejecting it.

/// Decode YAML 1.2 single-quoted scalar escapes (`''` -> `'`), returning
/// `None` if an unescaped lone `'` appears inside `raw`.
pub(super) fn unescape_yaml_single_quoted_strict(raw: &str) -> Option<String> {
    if !raw.contains('\'') {
        return Some(raw.to_owned());
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(character) = chars.next() {
        if character != '\'' {
            out.push(character);
            continue;
        }
        if chars.next() != Some('\'') {
            return None;
        }
        out.push('\'');
    }
    Some(out)
}

/// Decode YAML 1.2 double-quoted scalar escapes on a single-line slice,
/// returning `None` on an unescaped `"` or an unrecognized backslash escape.
pub(super) fn unescape_yaml_double_quoted_strict(raw: &str) -> Option<String> {
    if !raw.contains(['\\', '"']) {
        return Some(raw.to_owned());
    }
    let mut out = String::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        let character = raw[index..].chars().next()?;
        if character == '"' {
            return None;
        }
        if character != '\\' {
            out.push(character);
            index += character.len_utf8();
            continue;
        }
        let (decoded, consumed) = decode_double_quote_escape(&raw[index + 1..])?;
        out.push(decoded);
        index += 1 + consumed;
    }
    Some(out)
}

/// Decode YAML double-quoted escapes across an entire document so sentinel
/// prefix selection avoids prefixes synthesized via `\x70`, `\\\n`, etc.
pub(super) fn unescape_yaml_double_quoted_lenient(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            let character = text[index..]
                .chars()
                .next()
                .unwrap_or('\0');
            out.push(character);
            index += character.len_utf8();
            continue;
        }
        if let Some(consumed) = escaped_line_break_len(&bytes[index + 1..]) {
            index += 1 + consumed;
        } else if let Some((decoded, consumed)) = decode_double_quote_escape(&text[index + 1..]) {
            out.push(decoded);
            index += 1 + consumed;
        } else {
            out.push(' ');
            index += 1;
        }
    }
    out
}

fn decode_double_quote_escape(after_backslash: &str) -> Option<(char, usize)> {
    let first = *after_backslash.as_bytes().first()?;
    if let Some(decoded) = simple_double_quote_escape(first) {
        return Some((decoded, 1));
    }
    let hex_digits = match first {
        b'x' => 2,
        b'u' => 4,
        b'U' => 8,
        _ => return None,
    };
    let decoded = decode_hex_escape(&after_backslash[1..], hex_digits)?;
    Some((decoded, 1 + hex_digits))
}

fn simple_double_quote_escape(byte: u8) -> Option<char> {
    Some(match byte {
        b'0' => '\0',
        b'a' => '\x07',
        b'b' => '\x08',
        b't' | b'\t' => '\t',
        b'n' => '\n',
        b'v' => '\x0B',
        b'f' => '\x0C',
        b'r' => '\r',
        b'e' => '\x1B',
        b' ' => ' ',
        b'"' => '"',
        b'/' => '/',
        b'\\' => '\\',
        b'N' => '\u{85}',
        b'_' => '\u{A0}',
        b'L' => '\u{2028}',
        b'P' => '\u{2029}',
        _ => return None,
    })
}

fn decode_hex_escape(rest: &str, hex_digits: usize) -> Option<char> {
    let slice = rest.as_bytes().get(..hex_digits)?;
    if !slice.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let codepoint = u32::from_str_radix(&rest[..hex_digits], 16).ok()?;
    char::from_u32(codepoint)
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
