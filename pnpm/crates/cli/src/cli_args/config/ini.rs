//! Minimal flat-key INI read/modify/write for `.npmrc` and `auth.ini`.
//!
//! `pnpm config` reads and writes these files through the `ini` npm package
//! (`read-ini-file` / `write-ini-file`). The config keys it touches are flat
//! `key=value` pairs (`registry`, `@scope:registry`, `//host/:_authToken`,
//! `cafile`, ...) — no sections. The format supports arrays as repeated keys
//! (e.g., multiple `ca=` lines accumulate into an array), matching pnpm's INI
//! parser and the `NpmrcAuth::ca` field. Comments (`;` / `#`) and blank lines
//! are preserved where possible.

#[cfg(test)]
use indexmap::IndexMap;
use std::{fs, io, path::Path};

/// An INI file's content, parsed into lines and structured entries.
#[derive(Debug)]
struct IniContent {
    /// Lines in their original order: structured entries and unparsed lines
    /// (comments, blank lines, and malformed lines) interleaved.
    lines: Vec<Line>,
}

#[derive(Debug)]
enum Line {
    /// A `key=value` entry.
    Entry { key: String, raw: String },
    /// A comment, blank line, or malformed line, preserved verbatim.
    Unparsed(String),
}

/// Read `path` into an ordered key→value(s) map. A missing file is an empty
/// map; any other read error propagates. Repeated keys accumulate into the
/// value vector (e.g., multiple `ca=` lines).
#[cfg(test)]
pub fn read(path: &Path) -> io::Result<IndexMap<String, Vec<String>>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(parse(&text)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(IndexMap::new()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
fn parse(text: &str) -> IndexMap<String, Vec<String>> {
    let mut map = IndexMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            map.entry(key.trim().to_string())
                .or_insert_with(Vec::new)
                .push(value.trim().to_string());
        }
    }
    map
}

/// Parse `text` into structured lines, preserving comments and blank lines.
fn parse_with_structure(text: &str) -> IniContent {
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            lines.push(Line::Unparsed(line.to_string()));
        } else if let Some((key, _value)) = trimmed.split_once('=') {
            lines.push(Line::Entry { key: key.trim().to_string(), raw: line.to_string() });
        } else {
            lines.push(Line::Unparsed(line.to_string()));
        }
    }
    IniContent { lines }
}

/// Replace every entry for `key`, preserving unrelated lines in their original
/// order and spelling. Array values are written as repeated `key=value` lines.
pub fn write(path: &Path, key: &str, values: &[String]) -> io::Result<()> {
    let mut content = match fs::read_to_string(path) {
        Ok(text) => parse_with_structure(&text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => IniContent { lines: Vec::new() },
        Err(err) => return Err(err),
    };

    content.lines.retain(|line| match line {
        Line::Entry { key: entry_key, .. } => entry_key != key,
        Line::Unparsed(_) => true,
    });
    for value in values {
        content.lines.push(Line::Entry { key: key.to_string(), raw: format!("{key}={value}") });
    }
    write_content(path, &content)
}

fn write_content(path: &Path, content: &IniContent) -> io::Result<()> {
    let mut output = String::new();
    for line in &content.lines {
        match line {
            Line::Entry { raw, .. } | Line::Unparsed(raw) => {
                output.push_str(raw);
                output.push('\n');
            }
        }
    }
    pnpm_fs::write_atomic(path, output.as_bytes())
}

/// Remove every entry for `key` while preserving comments, blank lines, and
/// unrelated entries in their original order.
pub fn remove(path: &Path, key: &str) -> io::Result<bool> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    let mut content = parse_with_structure(&text);
    let original_len = content.lines.len();
    content
        .lines
        .retain(|line| !matches!(line, Line::Entry { key: entry_key, .. } if entry_key == key));
    if content.lines.len() == original_len {
        return Ok(false);
    }

    write_content(path, &content)?;
    Ok(true)
}
