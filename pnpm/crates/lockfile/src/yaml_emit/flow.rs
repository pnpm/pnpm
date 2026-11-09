//! The next format's syntax: JSON, with a trailing comma after every entry
//! of a multi-line collection. That is YAML flow style with every scalar
//! double-quoted, so a YAML reader parses the file as the same document and
//! `pnpm-lock.yaml` keeps its name and its readers, while a JSON reader that
//! allows the trailing comma parses it several times faster. The layout
//! follows the block emitter's: a blank line between the entries of the
//! top-level map and of `projects` and `packages`, the maps
//! [`one_line_values`] names on their keys' lines, and a map that
//! [names a snapshot](names_snapshot) one binding per line.

use super::{
    Format, INDENT, Nesting, PARALLEL_ENTRY_THRESHOLD, ValueStyle, is_single_line_key,
    is_single_line_map, names_snapshot, one_line_values,
};
use rayon::prelude::*;
use serde_json::{Map, Value};

/// Whether a collection spans lines, one entry per line with a trailing
/// comma, or sits on one line.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    Lines,
    OneLine,
}

/// Render a sorted next-format document.
pub(super) fn to_string(value: &Value) -> String {
    let mut out = String::new();
    let nesting = Nesting { format: Format::Next, values: ValueStyle::Block };
    write_value(&mut out, value, 0, Layout::Lines, None, nesting);
    out.push('\n');
    out
}

/// `key` is the map key the value is bound to, which drives the one-line
/// and blank-line decisions as it does in the block emitter.
fn write_value(
    out: &mut String,
    value: &Value,
    level: usize,
    layout: Layout,
    key: Option<&str>,
    nesting: Nesting,
) {
    match value {
        Value::Object(map) => write_map(out, map, level, layout, key, nesting),
        Value::Array(items) => write_list(out, items, level, layout, key, nesting.format),
        Value::String(text) => push_string(out, text),
        scalar => out.push_str(&scalar.to_string()),
    }
}

fn write_map(
    out: &mut String,
    map: &Map<String, Value>,
    level: usize,
    layout: Layout,
    key: Option<&str>,
    nesting: Nesting,
) {
    if map.is_empty() {
        out.push_str("{}");
        return;
    }
    if layout == Layout::OneLine || is_single_line_map(key, map) {
        write_one_line_map(out, map, level, nesting.format);
        return;
    }
    let blank_lines =
        level == 0 || matches!(key, Some("packages" | "importers" | "projects" | "snapshots"));
    let nesting = Nesting { values: one_line_values(key, nesting), ..nesting };
    out.push('{');
    if map.len() < PARALLEL_ENTRY_THRESHOLD {
        for (index, entry) in map.iter().enumerate() {
            separate(out, index, blank_lines);
            write_entry(out, entry, level, nesting);
        }
    } else {
        for (index, text) in render_entries(map, level, nesting).iter().enumerate() {
            separate(out, index, blank_lines);
            out.push_str(text);
        }
    }
    next_line(out, level);
    out.push('}');
}

/// The entries of a workspace-scale map, each rendered into its own buffer
/// on the pool; a small map appends straight into the one buffer instead.
fn render_entries(map: &Map<String, Value>, level: usize, nesting: Nesting) -> Vec<String> {
    map.iter()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|entry| {
            let mut text = String::new();
            write_entry(&mut text, *entry, level, nesting);
            text
        })
        .collect()
}

fn separate(out: &mut String, index: usize, blank_lines: bool) {
    if index > 0 && blank_lines {
        out.push('\n');
    }
}

/// One entry of a multi-line map, from its leading newline to its trailing
/// comma.
fn write_entry(
    out: &mut String,
    (entry_key, entry): (&String, &Value),
    level: usize,
    nesting: Nesting,
) {
    next_line(out, level + 1);
    push_string(out, entry_key);
    out.push_str(": ");
    let layout = match nesting.values {
        ValueStyle::Block | ValueStyle::FlowBelow => Layout::Lines,
        ValueStyle::Flow if names_snapshot(entry) => Layout::Lines,
        ValueStyle::Flow => Layout::OneLine,
    };
    write_value(out, entry, level + 1, layout, Some(entry_key), nesting);
    out.push(',');
}

fn write_one_line_map(out: &mut String, map: &Map<String, Value>, level: usize, format: Format) {
    out.push('{');
    for (index, (entry_key, entry)) in map.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        push_string(out, entry_key);
        out.push_str(": ");
        let nesting = Nesting { format, values: ValueStyle::Block };
        write_value(out, entry, level, Layout::OneLine, None, nesting);
    }
    out.push('}');
}

/// A list is one entry per line, except an empty one, one inside a one-line
/// map, and one under a key the block emitter writes on one line.
fn write_list(
    out: &mut String,
    items: &[Value],
    level: usize,
    layout: Layout,
    key: Option<&str>,
    format: Format,
) {
    let one_line =
        items.is_empty() || layout == Layout::OneLine || key.is_some_and(is_single_line_key);
    let nesting = Nesting { format, values: ValueStyle::Block };
    out.push('[');
    for (index, item) in items.iter().enumerate() {
        if one_line {
            if index > 0 {
                out.push_str(", ");
            }
            write_value(out, item, level, Layout::OneLine, None, nesting);
        } else {
            next_line(out, level + 1);
            write_value(out, item, level + 1, Layout::Lines, None, nesting);
            out.push(',');
        }
    }
    if !one_line {
        next_line(out, level);
    }
    out.push(']');
}

fn next_line(out: &mut String, level: usize) {
    out.push('\n');
    out.extend(std::iter::repeat_n(' ', INDENT * level));
}

/// `text` as a JSON string: `"` and `\` escaped, and a control character
/// as its escape or `\u00XX` form, as a JSON reader requires.
fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str(r#"\""#),
            '\\' => out.push_str(r"\\"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            control if control < ' ' => {
                use std::fmt::Write;
                write!(out, r"\u{:04x}", control as u32).expect("writing to a String");
            }
            other => out.push(other),
        }
    }
    out.push('"');
}
