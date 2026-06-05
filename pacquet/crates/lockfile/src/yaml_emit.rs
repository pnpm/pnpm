//! Byte-for-byte port of the YAML dumper pnpm uses for `pnpm-lock.yaml`.
//!
//! pnpm serializes its lockfile with [`@zkochan/js-yaml`], a fork of `js-yaml`
//! carrying lockfile-specific rendering rules that no general-purpose Rust YAML
//! serializer reproduces:
//!
//! - **`blankLines`** — a blank line separates the entries of the top-level map
//!   and of the `packages:` / `importers:` / `snapshots:` maps.
//! - **single-line keys** — `cpu`, `engines`, `os`, `libc`, and `resolution`
//!   (unless its `type` is `variations`/`binary`) render in flow style on one
//!   line; everything else renders in block style.
//! - **single-quote scalar style** — ambiguous scalars (`'9.0'`, `'>=10'`,
//!   `'@scope/name@1.0.0'`) are single-quoted, matching `js-yaml`'s default
//!   `quotingType`, not double-quoted.
//! - **`lineWidth: -1`, `noRefs: true`, `noCompatMode: true`** — no line
//!   wrapping, no anchors/aliases, no legacy-bool/base-60 quoting.
//!
//! The input is first lowered to a [`serde_json::Value`] (the crate enables
//! `serde_json/preserve_order`, so map order is retained), then its keys are
//! reordered by [`sort_lockfile_keys`] — a port of pnpm's `sortLockfileKeys`,
//! so the byte output is independent of pacquet's struct field order — and
//! finally rendered by a faithful translation of the fork's `dumper.js`.
//!
//! [`@zkochan/js-yaml`]: https://github.com/zkochan/js-yaml

use serde_json::{Map, Value};
use std::cmp::Ordering;

/// Keys whose collection value always renders on a single line (flow style).
/// Mirrors the fork's `SINGLE_LINE_KEYS`.
const SINGLE_LINE_KEYS: [&str; 4] = ["cpu", "engines", "os", "libc"];

/// One indentation level, in spaces (`js-yaml`'s default `indent`).
const INDENT: usize = 2;

/// Per-package / per-snapshot key priority. Mirrors `ORDERED_KEYS` in pnpm's
/// [`sortLockfileKeys`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/fs/src/sortLockfileKeys.ts).
const ORDERED_KEYS: [&str; 20] = [
    "resolution",
    "id",
    "name",
    "version",
    "engines",
    "cpu",
    "os",
    "libc",
    "deprecated",
    "hasBin",
    "prepare",
    "requiresBuild",
    "bundleDependencies",
    "peerDependencies",
    "peerDependenciesMeta",
    "dependencies",
    "optionalDependencies",
    "transitivePeerDependencies",
    "dev",
    "optional",
];

/// Top-level key priority. Mirrors `ROOT_KEYS` in pnpm's `sortLockfileKeys`.
const ROOT_KEYS: [&str; 9] = [
    "lockfileVersion",
    "settings",
    "catalogs",
    "overrides",
    "packageExtensionsChecksum",
    "pnpmfileChecksum",
    "patchedDependencies",
    "importers",
    "packages",
];

/// Serialize `value` to a YAML string matching pnpm's lockfile formatting.
pub(crate) fn to_string<Value: serde::Serialize>(
    value: &Value,
) -> Result<String, serde_json::Error> {
    let value = sort_lockfile_keys(serde_json::to_value(value)?);
    let mut dump = render(&value, 0, true, true, None, false);
    dump.push('\n');
    Ok(dump)
}

/// Reorder a lockfile document's keys to match pnpm's on-write ordering. Port
/// of pnpm's
/// [`sortLockfileKeys`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/fs/src/sortLockfileKeys.ts):
/// `importers` / `packages` / `snapshots` / `catalogs` / `time` /
/// `patchedDependencies` are sorted by their direct keys, each section's
/// entries are deep-sorted (by the priority map for packages/snapshots, by the
/// root priority for importers, lexically for catalogs), and finally the root
/// keys are ordered by priority.
fn sort_lockfile_keys(value: Value) -> Value {
    let Value::Object(mut root) = value else { return value };

    for (section, priority) in [
        ("importers", &ROOT_KEYS[..]),
        ("packages", &ORDERED_KEYS[..]),
        ("snapshots", &ORDERED_KEYS[..]),
    ] {
        if let Some(Value::Object(map)) = root.remove(section) {
            let sorted = map_values(sort_direct_keys(map), |entry| match entry {
                Value::Object(inner) => Value::Object(sort_by_priority(inner, priority, true)),
                other => other,
            });
            root.insert(section.to_string(), Value::Object(sorted));
        }
    }

    if let Some(Value::Object(catalogs)) = root.remove("catalogs") {
        let sorted = map_values(sort_direct_keys(catalogs), sort_deep_keys);
        root.insert("catalogs".to_string(), Value::Object(sorted));
    }

    for section in ["time", "patchedDependencies"] {
        if let Some(Value::Object(map)) = root.remove(section) {
            root.insert(section.to_string(), Value::Object(sort_direct_keys(map)));
        }
    }

    Value::Object(sort_by_priority(root, &ROOT_KEYS, false))
}

/// Plain code-unit key comparison, matching pnpm's `lexCompare`.
fn lex_cmp(left: &str, right: &str) -> Ordering {
    left.cmp(right)
}

/// Mirror of pnpm's `compareWithPriority`: prioritized keys come first in
/// priority order, the rest follow in `lexCompare` order.
fn priority_cmp(priority: &[&str], left: &str, right: &str) -> Ordering {
    let rank = |key: &str| priority.iter().position(|entry| *entry == key);
    match (rank(left), rank(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => lex_cmp(left, right),
    }
}

fn map_values(map: Map<String, Value>, transform: impl Fn(Value) -> Value) -> Map<String, Value> {
    map.into_iter().map(|(key, value)| (key, transform(value))).collect()
}

fn sort_direct_keys(map: Map<String, Value>) -> Map<String, Value> {
    sort_map(map, &lex_cmp, false)
}

fn sort_deep_keys(value: Value) -> Value {
    sort_value(value, &lex_cmp)
}

fn sort_by_priority(map: Map<String, Value>, priority: &[&str], deep: bool) -> Map<String, Value> {
    sort_map(map, &|left, right| priority_cmp(priority, left, right), deep)
}

fn sort_map(
    map: Map<String, Value>,
    compare: &dyn Fn(&str, &str) -> Ordering,
    deep: bool,
) -> Map<String, Value> {
    let mut entries: Vec<(String, Value)> = map.into_iter().collect();
    entries.sort_by(|(left, _), (right, _)| compare(left, right));
    entries
        .into_iter()
        .map(|(key, value)| (key, if deep { sort_value(value, compare) } else { value }))
        .collect()
}

/// Recursively sort the keys of every nested object with `compare`, recursing
/// through arrays without reordering their elements. Mirrors the `deep` option
/// of the `sort-keys` package pnpm uses.
fn sort_value(value: Value, compare: &dyn Fn(&str, &str) -> Ordering) -> Value {
    match value {
        Value::Object(map) => Value::Object(sort_map(map, compare, true)),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|item| sort_value(item, compare)).collect())
        }
        other => other,
    }
}

/// Render one node. Mirrors the fork's `writeNode`.
///
/// - `level` — current indentation depth.
/// - `block` — whether block style is permitted here (`false` inside flow).
/// - `compact` — whether the first child of a block collection omits its
///   leading newline (the collection sits on the same line as its key).
/// - `object_key` — the map key this value is bound to, driving the
///   single-line and blank-line decisions.
/// - `force_single_line` — propagated `singleLineOnly`: forces single-line
///   scalar styling for keys and for values nested in a single-line map.
fn render(
    value: &Value,
    level: usize,
    block: bool,
    compact: bool,
    object_key: Option<&str>,
    force_single_line: bool,
) -> String {
    match value {
        Value::Object(map) => {
            let single_line = is_single_line_map(object_key, map);
            if block && !map.is_empty() && !single_line {
                let double_line = level == 0
                    || matches!(object_key, Some("packages" | "importers" | "snapshots"));
                write_block_mapping(map, level, compact, double_line)
            } else {
                write_flow_mapping(map, level, single_line)
            }
        }
        Value::Array(seq) => {
            let single_line = object_key.is_some_and(is_single_line_key);
            if block && !seq.is_empty() && !single_line {
                write_block_sequence(seq, level, compact)
            } else {
                write_flow_sequence(seq, level)
            }
        }
        Value::String(string) => write_scalar(string, level, force_single_line, block),
        Value::Bool(boolean) => if *boolean { "true" } else { "false" }.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Null => "null".to_string(),
    }
}

fn is_single_line_key(key: &str) -> bool {
    SINGLE_LINE_KEYS.contains(&key)
}

/// Whether a map value renders on a single line. `resolution` is single-line
/// except for the nested `variations`/`binary` shapes (detected by the `type`
/// discriminator pnpm's tagged resolutions carry).
fn is_single_line_map(object_key: Option<&str>, map: &serde_json::Map<String, Value>) -> bool {
    match object_key {
        Some(key) if is_single_line_key(key) => true,
        Some("resolution") => {
            !matches!(map.get("type").and_then(Value::as_str), Some("variations" | "binary"))
        }
        _ => false,
    }
}

/// `generateNextLine`: a newline (doubled when `double_line`) plus this level's
/// indent.
fn next_line(level: usize, double_line: bool) -> String {
    let mut line = String::from("\n");
    if double_line {
        line.push('\n');
    }
    line.extend(std::iter::repeat_n(' ', INDENT * level));
    line
}

fn write_block_mapping(
    map: &serde_json::Map<String, Value>,
    level: usize,
    compact: bool,
    double_line: bool,
) -> String {
    let mut result = String::new();
    for (key, value) in map {
        if !compact || !result.is_empty() {
            result.push_str(&next_line(level, double_line));
        }
        result.push_str(&write_scalar(key, level + 1, true, true));
        let rendered = render(value, level + 1, true, false, Some(key), false);
        result.push(':');
        if !rendered.starts_with('\n') {
            result.push(' ');
        }
        result.push_str(&rendered);
    }
    if result.is_empty() { "{}".to_string() } else { result }
}

fn write_block_sequence(seq: &[Value], level: usize, compact: bool) -> String {
    let mut result = String::new();
    for value in seq {
        let rendered = render(value, level + 1, true, true, None, false);
        if !compact || !result.is_empty() {
            result.push_str(&next_line(level, false));
        }
        result.push('-');
        if !rendered.starts_with('\n') {
            result.push(' ');
        }
        result.push_str(&rendered);
    }
    if result.is_empty() { "[]".to_string() } else { result }
}

fn write_flow_mapping(
    map: &serde_json::Map<String, Value>,
    level: usize,
    single_line: bool,
) -> String {
    let mut result = String::new();
    for (key, value) in map {
        if !result.is_empty() {
            result.push_str(", ");
        }
        result.push_str(&write_scalar(key, level, single_line, false));
        result.push_str(": ");
        result.push_str(&render(value, level, false, false, None, single_line));
    }
    format!("{{{result}}}")
}

fn write_flow_sequence(seq: &[Value], level: usize) -> String {
    let mut result = String::new();
    for value in seq {
        if !result.is_empty() {
            result.push_str(", ");
        }
        result.push_str(&render(value, level, false, false, None, false));
    }
    format!("[{result}]")
}

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
fn write_scalar(string: &str, level: usize, single_line: bool, inblock: bool) -> String {
    if string.is_empty() {
        return "''".to_string();
    }
    match choose_scalar_style(string, single_line, inblock) {
        ScalarStyle::Plain => string.to_string(),
        ScalarStyle::Single => format!("'{}'", string.replace('\'', "''")),
        ScalarStyle::Double => format!("\"{}\"", escape_string(string)),
        ScalarStyle::Literal => {
            let indent = INDENT * level.max(1);
            format!(
                "|{}{}",
                block_header(string),
                drop_ending_newline(&indent_string(string, indent))
            )
        }
    }
}

/// Mirrors the fork's `chooseScalarStyle` under lockfile options.
fn choose_scalar_style(string: &str, single_line_only: bool, inblock: bool) -> ScalarStyle {
    let chars: Vec<u32> = string.chars().map(u32::from).collect();
    let mut plain = is_plain_safe_first(chars[0]) && is_plain_safe_last(chars[chars.len() - 1]);
    let mut has_line_break = false;
    let mut prev: Option<u32> = None;

    if single_line_only {
        for &char in &chars {
            if !is_printable(char) {
                return ScalarStyle::Double;
            }
            plain = plain && is_plain_safe(char, prev, inblock);
            prev = Some(char);
        }
    } else {
        for &char in &chars {
            if char == CHAR_LINE_FEED {
                has_line_break = true;
            } else if !is_printable(char) {
                return ScalarStyle::Double;
            }
            plain = plain && is_plain_safe(char, prev, inblock);
            prev = Some(char);
        }
    }

    if !has_line_break {
        if plain && !resolves_implicitly(string) {
            return ScalarStyle::Plain;
        }
        return ScalarStyle::Single;
    }
    ScalarStyle::Literal
}

const CHAR_TAB: u32 = 0x09;
const CHAR_LINE_FEED: u32 = 0x0A;
const CHAR_CARRIAGE_RETURN: u32 = 0x0D;
const CHAR_SPACE: u32 = 0x20;
const CHAR_SHARP: u32 = 0x23; // #
const CHAR_COLON: u32 = 0x3A; // :
const CHAR_COMMA: u32 = 0x2C; // ,
const CHAR_LEFT_SQUARE_BRACKET: u32 = 0x5B; // [
const CHAR_RIGHT_SQUARE_BRACKET: u32 = 0x5D; // ]
const CHAR_LEFT_CURLY_BRACKET: u32 = 0x7B; // {
const CHAR_RIGHT_CURLY_BRACKET: u32 = 0x7D; // }
const CHAR_BOM: u32 = 0xFEFF;

fn is_whitespace(c: u32) -> bool {
    c == CHAR_SPACE || c == CHAR_TAB
}

fn is_printable(c: u32) -> bool {
    (0x00020..=0x00007E).contains(&c)
        || ((0x000A1..=0x00D7FF).contains(&c) && c != 0x2028 && c != 0x2029)
        || ((0x0E000..=0x00FFFD).contains(&c) && c != CHAR_BOM)
        || (0x10000..=0x10FFFF).contains(&c)
}

fn is_ns_char_or_whitespace(c: u32) -> bool {
    is_printable(c) && c != CHAR_BOM && c != CHAR_CARRIAGE_RETURN && c != CHAR_LINE_FEED
}

fn is_plain_safe_first(c: u32) -> bool {
    is_printable(c)
        && c != CHAR_BOM
        && !is_whitespace(c)
        && !matches!(
            c,
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
            0x60 // `
        )
}

fn is_plain_safe_last(c: u32) -> bool {
    !is_whitespace(c) && c != CHAR_COLON
}

fn is_plain_safe(c: u32, prev: Option<u32>, inblock: bool) -> bool {
    let c_is_ns_or_ws = is_ns_char_or_whitespace(c);
    let c_is_ns = c_is_ns_or_ws && !is_whitespace(c);
    let base = if inblock {
        c_is_ns_or_ws
    } else {
        c_is_ns_or_ws
            && c != CHAR_COMMA
            && c != CHAR_LEFT_SQUARE_BRACKET
            && c != CHAR_RIGHT_SQUARE_BRACKET
            && c != CHAR_LEFT_CURLY_BRACKET
            && c != CHAR_RIGHT_CURLY_BRACKET
    };
    let prev_is_colon = prev == Some(CHAR_COLON);
    let prev_is_ns = prev.is_some_and(|p| is_ns_char_or_whitespace(p) && !is_whitespace(p));
    // change to true on '[^ ]#'
    if prev_is_ns && c == CHAR_SHARP {
        return true;
    }
    // change to true on ':[^ ]'
    if prev_is_colon && c_is_ns {
        return true;
    }
    // ns-plain-char: a non-`#` base character that isn't the `: ` sequence.
    base && c != CHAR_SHARP && (!prev_is_colon || c_is_ns)
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
        0x00 => "\\0",
        0x07 => "\\a",
        0x08 => "\\b",
        0x09 => "\\t",
        0x0A => "\\n",
        0x0B => "\\v",
        0x0C => "\\f",
        0x0D => "\\r",
        0x1B => "\\e",
        0x22 => "\\\"",
        0x5C => "\\\\",
        0x85 => "\\N",
        0xA0 => "\\_",
        0x2028 => "\\L",
        0x2029 => "\\P",
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
    format!("\\{handle}{:0>width$}", hex, width = width)
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
        match string[start..].find('\n') {
            Some(offset) => {
                lines.push(&string[start..start + offset + 1]);
                start += offset + 1;
            }
            None => {
                lines.push(&string[start..]);
                start = bytes.len();
            }
        }
    }
    lines
}

/// Whether a plain scalar would be reinterpreted as a non-string type and so
/// must be quoted. Mirrors `testImplicitResolving` over the default schema's
/// implicit types: null, bool, int, float, timestamp, merge.
fn resolves_implicitly(string: &str) -> bool {
    resolves_null(string)
        || resolves_bool(string)
        || resolves_int(string)
        || resolves_float(string)
        || resolves_timestamp(string)
        || string == "<<"
}

fn resolves_null(string: &str) -> bool {
    matches!(string, "~" | "null" | "Null" | "NULL")
}

fn resolves_bool(string: &str) -> bool {
    matches!(string, "true" | "True" | "TRUE" | "false" | "False" | "FALSE")
}

/// Port of `type/int.js`'s `resolveYamlInteger`.
fn resolves_int(string: &str) -> bool {
    let bytes = string.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut index = 0;
    if matches!(bytes[index], b'-' | b'+') {
        index += 1;
    }
    if index >= bytes.len() {
        return false;
    }
    if bytes[index] == b'0' {
        if index + 1 == bytes.len() {
            return true;
        }
        index += 1;
        match bytes[index] {
            b'b' => return digits_match(&bytes[index + 1..], |c| matches!(c, b'0' | b'1')),
            b'x' => return digits_match(&bytes[index + 1..], |c| c.is_ascii_hexdigit()),
            b'o' => return digits_match(&bytes[index + 1..], |c| matches!(c, b'0'..=b'7')),
            _ => {}
        }
    }
    if bytes[index] == b'_' {
        return false;
    }
    digits_match(&bytes[index..], |c| c.is_ascii_digit())
}

/// A run of `_`-separated digits accepted by `predicate`, with at least one
/// digit and no trailing `_`. Mirrors the per-base loops in `resolveYamlInteger`.
fn digits_match(bytes: &[u8], predicate: impl Fn(u8) -> bool) -> bool {
    let mut has_digits = false;
    let mut last = 0u8;
    for &byte in bytes {
        last = byte;
        if byte == b'_' {
            continue;
        }
        if !predicate(byte) {
            return false;
        }
        has_digits = true;
    }
    has_digits && last != b'_'
}

/// Port of `type/float.js`'s `YAML_FLOAT_PATTERN` test (with the trailing-`_`
/// guard).
fn resolves_float(string: &str) -> bool {
    if string.ends_with('_') {
        return false;
    }
    float_matches(string)
}

fn float_matches(string: &str) -> bool {
    // [-+]?.inf and .nan special forms.
    let unsigned = string.strip_prefix(['-', '+']).unwrap_or(string);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") || matches!(string, ".nan" | ".NaN" | ".NAN") {
        return true;
    }

    let body = string.strip_prefix(['-', '+']).unwrap_or(string);
    // Form A: [0-9][0-9_]* (\.[0-9_]*)? ([eE][-+]?[0-9]+)?
    // Form B: \.[0-9_]+ ([eE][-+]?[0-9]+)?  (no leading sign per the pattern).
    if let Some(after_dot) = body.strip_prefix('.') {
        if string.starts_with(['-', '+']) {
            return false;
        }
        let (mantissa, exponent) = split_exponent(after_dot);
        return !mantissa.is_empty()
            && mantissa.bytes().all(|b| b.is_ascii_digit() || b == b'_')
            && exponent_ok(exponent);
    }
    let mut cursor = body;
    let first = cursor.as_bytes().first().copied();
    if !first.is_some_and(|b| b.is_ascii_digit()) {
        return false;
    }
    // [0-9][0-9_]*
    let int_len = cursor.bytes().take_while(|b| b.is_ascii_digit() || *b == b'_').count();
    cursor = &cursor[int_len..];
    // (\.[0-9_]*)?
    if let Some(rest) = cursor.strip_prefix('.') {
        let frac_len = rest.bytes().take_while(|b| b.is_ascii_digit() || *b == b'_').count();
        cursor = &rest[frac_len..];
    }
    // ([eE][-+]?[0-9]+)? — and nothing left over.
    exponent_consumes_all(cursor)
}

/// Split a float mantissa from its optional `[eE]...` exponent.
fn split_exponent(string: &str) -> (&str, Option<&str>) {
    match string.find(['e', 'E']) {
        Some(index) => (&string[..index], Some(&string[index..])),
        None => (string, None),
    }
}

/// Whether an optional exponent tail (`""` or `[eE][-+]?[0-9]+`) is valid.
fn exponent_ok(exponent: Option<&str>) -> bool {
    match exponent {
        None | Some("") => true,
        Some(tail) => exponent_consumes_all(tail),
    }
}

fn exponent_consumes_all(tail: &str) -> bool {
    if tail.is_empty() {
        return true;
    }
    let Some(rest) = tail.strip_prefix(['e', 'E']) else {
        return false;
    };
    let rest = rest.strip_prefix(['-', '+']).unwrap_or(rest);
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
}

/// Port of `type/timestamp.js`'s `resolveYamlTimestamp` (date and full forms).
fn resolves_timestamp(string: &str) -> bool {
    matches_date(string) || matches_timestamp(string)
}

fn matches_date(string: &str) -> bool {
    let bytes = string.as_bytes();
    bytes.len() == 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

fn matches_timestamp(string: &str) -> bool {
    let bytes = string.as_bytes();
    let mut i = 0;
    let take_digits = |bytes: &[u8], i: &mut usize, min: usize, max: usize| -> bool {
        let start = *i;
        while *i < bytes.len() && *i - start < max && bytes[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - start >= min
    };
    if !take_digits(bytes, &mut i, 4, 4) {
        return false;
    }
    if bytes.get(i) != Some(&b'-') {
        return false;
    }
    i += 1;
    if !take_digits(bytes, &mut i, 1, 2) {
        return false;
    }
    if bytes.get(i) != Some(&b'-') {
        return false;
    }
    i += 1;
    if !take_digits(bytes, &mut i, 1, 2) {
        return false;
    }
    // (?:[Tt]|[ \t]+)
    match bytes.get(i) {
        Some(b'T' | b't') => i += 1,
        Some(b' ' | b'\t') => {
            while matches!(bytes.get(i), Some(b' ' | b'\t')) {
                i += 1;
            }
        }
        _ => return false,
    }
    if !take_digits(bytes, &mut i, 1, 2) {
        return false;
    }
    if bytes.get(i) != Some(&b':') {
        return false;
    }
    i += 1;
    if !take_digits(bytes, &mut i, 2, 2) {
        return false;
    }
    if bytes.get(i) != Some(&b':') {
        return false;
    }
    i += 1;
    if !take_digits(bytes, &mut i, 2, 2) {
        return false;
    }
    // (?:\.([0-9]*))?
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while matches!(bytes.get(i), Some(b) if b.is_ascii_digit()) {
            i += 1;
        }
    }
    // (?:[ \t]*(Z|([-+])([0-9][0-9]?)(?::([0-9][0-9]))?))?
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    if i == bytes.len() {
        return true;
    }
    match bytes.get(i) {
        Some(b'Z') => {
            i += 1;
        }
        Some(b'-' | b'+') => {
            i += 1;
            if !take_digits(bytes, &mut i, 1, 2) {
                return false;
            }
            if bytes.get(i) == Some(&b':') {
                i += 1;
                if !take_digits(bytes, &mut i, 2, 2) {
                    return false;
                }
            }
        }
        _ => return false,
    }
    i == bytes.len()
}

#[cfg(test)]
mod tests {
    use super::to_string;
    use serde_json::json;

    #[test]
    fn version_like_floats_are_single_quoted() {
        // `9.0` resolves as a YAML float, so it must be quoted; `11.5.2` and
        // `1.0.0` are plain strings.
        let yaml = to_string(&json!({ "a": "9.0", "b": "11.5.2", "c": "1.0.0" })).unwrap();
        // Top-level entries are blank-line separated.
        assert_eq!(yaml, "a: '9.0'\n\nb: 11.5.2\n\nc: 1.0.0\n");
    }

    #[test]
    fn leading_indicator_and_at_force_single_quotes() {
        let yaml = to_string(&json!({
            "node": ">=10",
            "name": "@scope/pkg@1.0.0",
        }))
        .unwrap();
        // Keys are sorted (`name` before `node`).
        assert_eq!(yaml, "name: '@scope/pkg@1.0.0'\n\nnode: '>=10'\n");
    }

    #[test]
    fn booleans_and_numbers_render_plain() {
        let yaml = to_string(&json!({ "hasBin": true, "max": 1000 })).unwrap();
        assert_eq!(yaml, "hasBin: true\n\nmax: 1000\n");
    }

    #[test]
    fn ambiguous_words_are_quoted() {
        let yaml = to_string(&json!({ "a": "true", "b": "null", "c": "yes", "d": "no" })).unwrap();
        // `true`/`null` are reserved words and get quoted; `yes`/`no` are plain
        // in the core schema (no legacy-bool resolving), matching pnpm's
        // noCompatMode dumper.
        assert_eq!(yaml, "a: 'true'\n\nb: 'null'\n\nc: yes\n\nd: no\n");
    }

    #[test]
    fn blank_lines_between_top_level_and_named_section_entries() {
        let yaml = to_string(&json!({
            "lockfileVersion": "9.0",
            "packages": {
                "a@1.0.0": { "x": 1 },
                "b@2.0.0": { "y": 2 },
            },
        }))
        .unwrap();
        assert_eq!(
            yaml,
            "lockfileVersion: '9.0'\n\npackages:\n\n  a@1.0.0:\n    x: 1\n\n  b@2.0.0:\n    y: 2\n"
        );
    }

    #[test]
    fn single_line_keys_render_flow() {
        let yaml = to_string(&json!({
            "resolution": { "integrity": "sha512-abc" },
            "engines": { "node": ">=10" },
            "cpu": ["x64"],
            "os": ["darwin", "linux"],
        }))
        .unwrap();
        // Keys are sorted (cpu, engines, os, resolution); array elements keep
        // their order.
        assert_eq!(
            yaml,
            "cpu: [x64]\n\nengines: {node: '>=10'}\n\nos: [darwin, linux]\n\nresolution: {integrity: sha512-abc}\n"
        );
    }

    #[test]
    fn variations_resolution_renders_block_not_flow() {
        // `resolution` is single-line except when its `type` is variations/binary.
        let yaml = to_string(&json!({
            "resolution": { "type": "variations", "variants": ["a"] },
        }))
        .unwrap();
        assert_eq!(yaml, "resolution:\n  type: variations\n  variants:\n    - a\n");
    }

    #[test]
    fn nan_resolves_as_float() {
        assert!(super::resolves_float(".nan"));
        assert!(super::resolves_float("-.inf"));
        assert!(super::resolves_float("3.14"));
        assert!(!super::resolves_float("3.14.15"));
    }

    #[test]
    fn timestamp_strings_are_quoted() {
        let yaml = to_string(&json!({ "t": "2021-01-01" })).unwrap();
        assert_eq!(yaml, "t: '2021-01-01'\n");
    }
}
