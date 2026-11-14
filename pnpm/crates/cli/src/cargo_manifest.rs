pub(crate) mod workspace;

pub(crate) use workspace::{workspace_dependency_spec, workspace_includes_member};

mod spans;

use miette::{IntoDiagnostic, Result, WrapErr};
use spans::{
    apply_two_edits, collect_header_spans, dotted_prefix, end_of_line, find_entry, full_line_range,
    inline_entry_removal_range, insert_into_inline_table, insert_line_at, is_explicit_header,
    is_inline_table, lookup_table_path, max_value_end, replace_range, section_end,
};
use std::{fs, ops::Range, path::Path};
use toml::{
    Spanned,
    de::{DeTable, DeValue},
};

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
    let doc = DeTable::parse(contents).into_diagnostic().wrap_err("parse Cargo.toml")?;
    let mut header_spans = Vec::new();
    collect_header_spans(doc.get_ref(), contents, &mut header_spans);
    header_spans.sort_by_key(|span| span.start);

    let Some(target) = lookup_table_path(&doc, table) else {
        return Ok(append_new_table(contents, table, name, version_spec));
    };
    let deps_table = target
        .get_ref()
        .as_table()
        .ok_or_else(|| miette::miette!("`{table}` in Cargo.toml is not a table"))?;

    if let Some((_, dep_val)) = find_entry(deps_table, name) {
        return update_existing_dependency(contents, &header_spans, dep_val, name, version_spec);
    }

    let entry = format!("{} = {}", format_key(name), quoted(version_spec));
    let target_span = target.span();
    if is_explicit_header(contents, &target_span) {
        let end = section_end(contents, &header_spans, &target_span);
        return Ok(insert_line_at(contents, end, &entry));
    }
    if is_inline_table(contents, &target_span) {
        return Ok(insert_into_inline_table(contents, &target_span, deps_table, &entry));
    }
    if header_spans
        .iter()
        .any(|header| header.start <= target_span.start && target_span.end <= header.end)
    {
        return Ok(append_new_table(contents, table, name, version_spec));
    }
    let prefix = dotted_prefix(contents, &target_span);
    let after = end_of_line(contents, max_value_end(deps_table).unwrap_or(target_span.end));
    Ok(insert_line_at(contents, after, &format!("{prefix}.{entry}")))
}

fn update_existing_dependency(
    contents: &str,
    header_spans: &[Range<usize>],
    dep_val: &Spanned<DeValue<'_>>,
    name: &str,
    version_spec: &str,
) -> Result<String> {
    match dep_val.get_ref() {
        DeValue::String(_) => Ok(replace_range(contents, dep_val.span(), &quoted(version_spec))),
        DeValue::Table(subtable) => {
            let span = dep_val.span();
            let shape = if is_inline_table(contents, &span) {
                TableShape::Inline
            } else if is_explicit_header(contents, &span) {
                TableShape::Subtable(header_spans)
            } else {
                TableShape::Dotted
            };
            update_table_dependency(contents, &span, subtable, shape, name, version_spec)
        }
        _ => Err(miette::miette!(
            "cannot update crate {name}: its Cargo.toml dependency declaration is not a string or table"
        )),
    }
}

#[derive(Clone, Copy)]
enum TableShape<'a> {
    Inline,
    Subtable(&'a [Range<usize>]),
    Dotted,
}

fn update_table_dependency(
    contents: &str,
    span: &Range<usize>,
    subtable: &DeTable<'_>,
    shape: TableShape<'_>,
    name: &str,
    version_spec: &str,
) -> Result<String> {
    match (find_entry(subtable, "version"), find_entry(subtable, "workspace")) {
        (Some((_, ver_val)), None) => {
            ensure_string_version(ver_val, name)?;
            Ok(replace_range(contents, ver_val.span(), &quoted(version_spec)))
        }
        (None, Some((ws_key, ws_val))) => Ok(replace_range(
            contents,
            ws_key.span().start..ws_val.span().end,
            &format!("version = {}", quoted(version_spec)),
        )),
        (Some((_, ver_val)), Some((ws_key, ws_val))) => {
            ensure_string_version(ver_val, name)?;
            let ws_span = ws_key.span().start..ws_val.span().end;
            let removal = match shape {
                TableShape::Inline => inline_entry_removal_range(contents, span, subtable, ws_span),
                TableShape::Subtable(_) | TableShape::Dotted => full_line_range(contents, ws_span),
            };
            Ok(apply_two_edits(contents, (ver_val.span(), &quoted(version_spec)), (removal, "")))
        }
        (None, None) => insert_missing_version(contents, span, subtable, shape, version_spec),
    }
}

fn insert_missing_version(
    contents: &str,
    span: &Range<usize>,
    subtable: &DeTable<'_>,
    shape: TableShape<'_>,
    version_spec: &str,
) -> Result<String> {
    let version_entry = format!("version = {}", quoted(version_spec));
    match shape {
        TableShape::Inline => {
            Ok(insert_into_inline_table(contents, span, subtable, &version_entry))
        }
        TableShape::Subtable(header_spans) => {
            let end = section_end(contents, header_spans, span);
            Ok(insert_line_at(contents, end, &version_entry))
        }
        TableShape::Dotted => {
            let prefix = dotted_prefix(contents, span);
            let after = end_of_line(contents, max_value_end(subtable).unwrap_or(span.end));
            Ok(insert_line_at(contents, after, &format!("{prefix}.{version_entry}")))
        }
    }
}

fn ensure_string_version(value: &Spanned<DeValue<'_>>, name: &str) -> Result<()> {
    if matches!(value.get_ref(), DeValue::String(_)) {
        Ok(())
    } else {
        Err(miette::miette!(
            "cannot update crate {name}: its Cargo.toml version declaration is not a string"
        ))
    }
}

fn append_new_table(contents: &str, table: &str, name: &str, version_spec: &str) -> String {
    let separator = appended_table_separator(contents);
    format!("{contents}{separator}[{table}]\n{} = {}\n", format_key(name), quoted(version_spec))
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

fn format_key(name: &str) -> String {
    if !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        name.to_string()
    } else {
        quoted(name)
    }
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a Rust string cannot fail")
}

#[cfg(test)]
mod tests;
