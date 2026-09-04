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

fn upsert_dependency(
    contents: &str,
    table: &str,
    name: &str,
    version_spec: &str,
) -> Result<String> {
    let Some((section_start, section_end)) = find_table(contents, table) else {
        let separator = if contents.is_empty() || contents.ends_with("\n\n") {
            ""
        } else if contents.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        return Ok(format!("{contents}{separator}[{table}]\n{name} = {}\n", quoted(version_spec)));
    };

    let section = &contents[section_start..section_end];
    let mut offset = section_start;
    for line in section.split_inclusive('\n') {
        if let Some(value_range) = dependency_value_range(line, name) {
            let value_range = (offset + value_range.start)..(offset + value_range.end);
            return replace_dependency_value(contents, value_range, name, version_spec);
        }
        offset += line.len();
    }

    let prefix = &contents[..section_end];
    let newline = if !prefix.is_empty() && !prefix.ends_with('\n') { "\n" } else { "" };
    Ok(format!("{prefix}{newline}{name} = {}\n{}", quoted(version_spec), &contents[section_end..]))
}

fn find_table(contents: &str, table: &str) -> Option<(usize, usize)> {
    let wanted = format!("[{table}]");
    let mut section_start = None;
    let mut offset = 0;
    for line in contents.split_inclusive('\n') {
        let trimmed = line.trim();
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

fn dependency_value_range(line: &str, name: &str) -> Option<Range<usize>> {
    let content_start = line.len() - line.trim_start().len();
    let content = &line[content_start..];
    if content.starts_with('#') {
        return None;
    }
    let equals = content.find('=')?;
    let key = content[..equals].trim();
    let key = key
        .strip_prefix('"')
        .and_then(|key| key.strip_suffix('"'))
        .or_else(|| key.strip_prefix('\'').and_then(|key| key.strip_suffix('\'')))
        .unwrap_or(key);
    if key != name {
        return None;
    }
    let after_equals = content_start + equals + 1;
    let whitespace = line[after_equals..].len() - line[after_equals..].trim_start().len();
    let start = after_equals + whitespace;
    let end = comment_start(&line[start..])
        .map_or_else(|| line.trim_end_matches(['\r', '\n']).len(), |comment| start + comment);
    Some(start..end - line[..end].len().saturating_sub(line[..end].trim_end().len()))
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
            "cannot update crate {name}: its Cargo.toml dependency declaration is not a string or single-line inline table"
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

fn inline_version_range(value: &str) -> Option<Range<usize>> {
    let bytes = value.as_bytes();
    let mut cursor = 0;
    let mut quote = None;
    let mut escaped = false;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if escaped {
            escaped = false;
            cursor += 1;
            continue;
        }
        if let Some(active_quote) = quote {
            if active_quote == b'"' && byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
            cursor += 1;
            continue;
        }
        if matches!(byte, b'"' | b'\'') {
            quote = Some(byte);
            cursor += 1;
            continue;
        }
        if !is_identifier_byte(byte) {
            cursor += 1;
            continue;
        }
        let key_start = cursor;
        while bytes.get(cursor).is_some_and(|byte| is_identifier_byte(*byte)) {
            cursor += 1;
        }
        if &value[key_start..cursor] != "version" {
            continue;
        }
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'=') {
            continue;
        }
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let active_quote = *bytes.get(cursor)?;
        if !matches!(active_quote, b'"' | b'\'') {
            continue;
        }
        let string_start = cursor;
        cursor += 1;
        let mut escaped = false;
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            cursor += 1;
            if escaped {
                escaped = false;
            } else if active_quote == b'"' && byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                return Some(string_start..cursor);
            }
        }
        return None;
    }
    None
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a Rust string cannot fail")
}

#[cfg(test)]
mod tests;
