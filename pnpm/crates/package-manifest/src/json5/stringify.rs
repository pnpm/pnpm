use std::fmt::Write;

use serde_json::{Map, Value};

/// Render `value` as JSON5.
///
/// A non-empty `indent` pretty-prints with a trailing comma. Identifier keys
/// stay bare. Strings use the quote that needs less escaping, and a tie uses
/// single quotes. Non-ASCII keys are quoted, which is valid JSON5 and avoids
/// the Unicode identifier tables.
pub(crate) fn stringify(value: &Value, indent: &str) -> String {
    let mut out = String::new();
    write_value(&mut out, value, indent_unit(indent), 0);
    out
}

fn indent_unit(indent: &str) -> &str {
    match indent.char_indices().nth(10) {
        Some((end, _)) => &indent[..end],
        None => indent,
    }
}

fn write_value(out: &mut String, value: &Value, gap: &str, depth: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => write_string(out, text),
        Value::Array(items) => write_array(out, items, gap, depth),
        Value::Object(fields) => write_object(out, fields, gap, depth),
    }
}

fn write_object(out: &mut String, fields: &Map<String, Value>, gap: &str, depth: usize) {
    if fields.is_empty() {
        out.push_str("{}");
        return;
    }
    out.push('{');
    let mut first = true;
    for (key, value) in fields {
        write_separator(out, gap, depth, &mut first);
        write_key(out, key);
        if gap.is_empty() {
            out.push(':');
        } else {
            out.push_str(": ");
        }
        write_value(out, value, gap, depth + 1);
    }
    write_closer(out, gap, depth, '}');
}

fn write_array(out: &mut String, items: &[Value], gap: &str, depth: usize) {
    if items.is_empty() {
        out.push_str("[]");
        return;
    }
    out.push('[');
    let mut first = true;
    for item in items {
        write_separator(out, gap, depth, &mut first);
        write_value(out, item, gap, depth + 1);
    }
    write_closer(out, gap, depth, ']');
}

fn write_separator(out: &mut String, gap: &str, depth: usize, first: &mut bool) {
    if !*first {
        out.push(',');
    }
    if !gap.is_empty() {
        out.push('\n');
        write_indent(out, gap, depth + 1);
    }
    *first = false;
}

fn write_closer(out: &mut String, gap: &str, depth: usize, close: char) {
    if gap.is_empty() {
        out.push(close);
        return;
    }
    out.push_str(",\n");
    write_indent(out, gap, depth);
    out.push(close);
}

fn write_indent(out: &mut String, gap: &str, depth: usize) {
    for _ in 0..depth {
        out.push_str(gap);
    }
}

fn write_key(out: &mut String, key: &str) {
    if is_identifier(key) {
        out.push_str(key);
    } else {
        write_string(out, key);
    }
}

fn is_identifier(key: &str) -> bool {
    let mut characters = key.chars();
    match characters.next() {
        Some(first) if is_id_start(first) => characters.all(is_id_continue),
        _ => false,
    }
}

fn is_id_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '$' || character == '_'
}

fn is_id_continue(character: char) -> bool {
    is_id_start(character) || character.is_ascii_digit()
}

fn write_string(out: &mut String, value: &str) {
    let mut single_quotes = 0usize;
    let mut double_quotes = 0usize;
    for character in value.chars() {
        match character {
            '\'' => single_quotes += 1,
            '"' => double_quotes += 1,
            _ => {}
        }
    }
    let quote = if single_quotes > double_quotes { '"' } else { '\'' };
    out.push(quote);
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            character if character == quote => {
                out.push('\\');
                out.push(character);
            }
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            '\0' if characters.peek().is_some_and(char::is_ascii_digit) => out.push_str("\\x00"),
            '\0' => out.push_str("\\0"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            character if (character as u32) < 0x20 => {
                write!(out, "\\x{:02x}", character as u32).expect("string formatting succeeds");
            }
            character => out.push(character),
        }
    }
    out.push(quote);
}

#[cfg(test)]
mod tests;
