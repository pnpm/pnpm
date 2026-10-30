pub(crate) mod workspace;

pub(crate) use workspace::{workspace_dependency_spec, workspace_includes_member};

use miette::{IntoDiagnostic, Result, WrapErr};
use std::{fs, ops::Range, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CargoDependencyKind {
    Normal,
    Development,
    Build,
}

impl CargoDependencyKind {
    fn table(self) -> &'static str {
        match self {
            Self::Normal => "dependencies",
            Self::Development => "dev-dependencies",
            Self::Build => "build-dependencies",
        }
    }
}

pub(crate) fn add_dependencies(
    manifest_path: &Path,
    dependencies: &[(String, String)],
    kind: CargoDependencyKind,
) -> Result<()> {
    let original = fs::read_to_string(manifest_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", manifest_path.display()))?;
    let mut updated = original.clone();
    for (name, version_spec) in dependencies {
        updated = upsert_dependency(&updated, kind.table(), name, version_spec)?;
    }
    if updated != original {
        pnpm_fs::write_atomic(manifest_path, updated.as_bytes())
            .into_diagnostic()
            .wrap_err_with(|| format!("write {}", manifest_path.display()))?;
    }
    Ok(())
}

pub(crate) fn upsert_dependency(
    contents: &str,
    table: &str,
    name: &str,
    version_spec: &str,
) -> Result<String> {
    if let Some(subtable) = find_dependency_subtable(contents, table, name) {
        return upsert_dependency(contents, &subtable, "version", version_spec);
    }
    let Some((section_start, section_end)) = find_table(contents, table) else {
        let separator = appended_table_separator(contents);
        return Ok(format!("{contents}{separator}[{table}]\n{name} = {}\n", quoted(version_spec)));
    };

    let section = &contents[section_start..section_end];
    let mut offset = section_start;
    for line in section.split_inclusive('\n') {
        if let Some(value_range) = dependency_value_range(line, name) {
            let value_range = value_range_in_contents(contents, offset, value_range);
            return replace_dependency_value(contents, value_range, name, version_spec);
        }
        offset += line.len();
    }

    let prefix = &contents[..section_end];
    let newline = if !prefix.is_empty() && !prefix.ends_with('\n') { "\n" } else { "" };
    Ok(format!("{prefix}{newline}{name} = {}\n{}", quoted(version_spec), &contents[section_end..]))
}

/// Translate a value range found on the line at `line_offset` into
/// `contents`, extending an inline table to its closing `}` on a later line.
fn value_range_in_contents(
    contents: &str,
    line_offset: usize,
    value_range: Range<usize>,
) -> Range<usize> {
    let start = line_offset + value_range.start;
    match inline_table_len(&contents[start..]) {
        Some(len) => start..start + len,
        None => start..line_offset + value_range.end,
    }
}

fn find_dependency_subtable(contents: &str, table: &str, name: &str) -> Option<String> {
    [format!("{table}.{name}"), format!("{table}.{}", quoted(name)), format!("{table}.'{name}'")]
        .into_iter()
        .find(|subtable| find_table(contents, subtable).is_some())
}

/// What has to come between the existing contents and a table appended
/// to them: nothing when the file already ends in a blank line, and
/// otherwise enough newlines to make one.
fn appended_table_separator(contents: &str) -> &'static str {
    if contents.is_empty() || contents.ends_with("\n\n") {
        ""
    } else if contents.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    }
}

fn find_table(contents: &str, table: &str) -> Option<(usize, usize)> {
    let wanted = format!("[{table}]");
    let mut section_start = None;
    let mut offset = 0;
    for line in contents.split_inclusive('\n') {
        let trimmed = line.trim();
        let trimmed = comment_start(trimmed)
            .map_or(trimmed, |start| &trimmed[..start])
            .trim_end();
        if section_start.is_some() && is_table_header(trimmed) {
            return section_start.map(|start| (start, offset));
        }
        if trimmed == wanted {
            section_start = Some(offset + line.len());
        }
        offset += line.len();
    }
    section_start.map(|start| (start, contents.len()))
}

fn is_table_header(line: &str) -> bool {
    line.starts_with('[') && line.ends_with(']')
}

fn unquote_key(key: &str) -> &str {
    key.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| {
            key.strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
        })
        .unwrap_or(key)
}

fn matches_dependency_key(raw_key: &str, name: &str) -> bool {
    unquote_key(raw_key) == name
        || raw_key
            .rsplit_once('.')
            .is_some_and(|(prefix, suffix)| {
                unquote_key(prefix.trim()) == name && unquote_key(suffix.trim()) == "version"
            })
}

fn dependency_value_range(line: &str, name: &str) -> Option<Range<usize>> {
    let content_start = line.len() - line.trim_start().len();
    let content = &line[content_start..];
    if content.starts_with('#') {
        return None;
    }
    let equals = content.find('=')?;
    if !matches_dependency_key(content[..equals].trim(), name) {
        return None;
    }
    let after_equals = content_start + equals + 1;
    let whitespace = line[after_equals..].len() - line[after_equals..].trim_start().len();
    let start = after_equals + whitespace;
    let end = match comment_start(&line[start..]) {
        Some(comment) => start + comment,
        None => line
            .trim_end_matches(['\r', '\n'])
            .len(),
    };
    Some(
        start
            ..end
                - line[..end]
                    .len()
                    .saturating_sub(line[..end].trim_end().len()),
    )
}

fn comment_start(value: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match (quote, character) {
            (Some('"'), '\\') => escaped = true,
            (Some(active), current) if active == current => quote = None,
            (None, '"' | '\'') => quote = Some(character),
            (None, '#') => return Some(index),
            _ => {}
        }
    }
    None
}

fn replace_dependency_value(
    contents: &str,
    value_range: Range<usize>,
    name: &str,
    version_spec: &str,
) -> Result<String> {
    let value = contents[value_range.clone()].trim();
    let replacement = if is_quoted(value) {
        quoted(version_spec)
    } else if value.starts_with('{') && value.ends_with('}') {
        replace_inline_version(value, name, version_spec)?
    } else {
        return Err(miette::miette!(
            "cannot update crate {name}: its Cargo.toml dependency declaration is not a string or inline table"
        ));
    };
    Ok(
        format!(
            "{}{}{}",
            &contents[..value_range.start],
            replacement,
            &contents[value_range.end..],
        ),
    )
}

fn is_quoted(value: &str) -> bool {
    (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
}

fn replace_inline_version(value: &str, name: &str, version_spec: &str) -> Result<String> {
    let version_range = inline_version_range(value).ok_or_else(|| {
        miette::miette!(
            "cannot update crate {name}: its inline dependency declaration has no string version"
        )
    })?;
    Ok(format!(
        "{}{}{}",
        &value[..version_range.start],
        quoted(version_spec),
        &value[version_range.end..],
    ))
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

/// Length of the inline table at the start of `value`, through its
/// closing `}`, which may sit on a later line. `None` when `value` does
/// not start with `{` or the table never closes.
fn inline_table_len(value: &str) -> Option<usize> {
    if !value.starts_with('{') {
        return None;
    }
    let mut scan = InlineScan { bytes: value.as_bytes(), cursor: 0 };
    let mut depth = 0usize;
    while let Some(byte) = scan.peek() {
        match byte {
            b'"' | b'\'' => {
                scan.take_string()?;
                continue;
            }
            b'#' => {
                scan.skip_comment();
                continue;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(scan.cursor + 1);
                }
            }
            _ => {}
        }
        scan.cursor += 1;
    }
    None
}

fn inline_version_range(value: &str) -> Option<Range<usize>> {
    let mut scan = InlineScan { bytes: value.as_bytes(), cursor: 0 };
    while let Some(byte) = scan.peek() {
        // A quoted value or a comment may contain anything, `version =` included.
        if matches!(byte, b'"' | b'\'') {
            scan.take_string()?;
            continue;
        }
        if byte == b'#' {
            scan.skip_comment();
            continue;
        }
        if !is_identifier_byte(byte) {
            scan.cursor += 1;
            continue;
        }
        let key = scan.take_identifier();
        if &value[key] != "version" || !scan.at_quoted_value() {
            continue;
        }
        return scan.take_string();
    }
    None
}

/// Byte scanner over one dependency declaration's inline table.
struct InlineScan<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl InlineScan<'_> {
    /// Step over the quoted string at the cursor, leaving the cursor
    /// past its closing quote. The returned range covers both quotes.
    /// `None` when the string never closes.
    fn take_string(&mut self) -> Option<Range<usize>> {
        let quote = self.peek()?;
        let start = self.cursor;
        self.cursor += 1;
        let mut escaped = false;
        while let Some(byte) = self.peek() {
            self.cursor += 1;
            if escaped {
                escaped = false;
            } else if quote == b'"' && byte == b'\\' {
                escaped = true;
            } else if byte == quote {
                return Some(start..self.cursor);
            }
        }
        None
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.cursor).copied()
    }

    fn take_identifier(&mut self) -> Range<usize> {
        let start = self.cursor;
        while self.peek().is_some_and(is_identifier_byte) {
            self.cursor += 1;
        }
        start..self.cursor
    }

    /// Step over the `=` that follows a key, and report whether the
    /// value after it is a quoted string.
    fn at_quoted_value(&mut self) -> bool {
        self.skip_whitespace();
        if self.peek() != Some(b'=') {
            return false;
        }
        self.cursor += 1;
        self.skip_whitespace();
        matches!(self.peek(), Some(b'"' | b'\''))
    }
    /// Step to the end of the line the cursor's comment is on.
    fn skip_comment(&mut self) {
        while self
            .peek()
            .is_some_and(|byte| byte != b'\n')
        {
            self.cursor += 1;
        }
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.cursor += 1;
        }
    }
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a Rust string cannot fail")
}

#[cfg(test)]
mod tests;
