use super::{
    inheritance::{ResolvedWorkspaceDep, lookup_workspace_dep_for_detach, parse_workspace_flag},
    spans::{
        find_entry, full_line_range, inline_entry_removal_range, is_inline_table, lookup_table_path,
    },
};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use std::{ops::Range, path::Path};
use toml::{
    Spanned,
    de::{DeTable, DeValue},
};

#[derive(Clone, Copy)]
enum PackageRetention<'a> {
    KeepMember,
    MatchExpected(Option<&'a str>),
}

pub(super) struct PreparedDetach {
    pub(super) sanitized_contents: String,
    pub(super) resolved_ws: Option<ResolvedWorkspaceDep>,
}

pub(super) fn prepare_workspace_detach(
    contents: &str,
    manifest_dir: Option<&Path>,
    table: &str,
    name: &str,
) -> Result<Option<PreparedDetach>> {
    let doc = DeTable::parse(contents).into_diagnostic().wrap_err("parse Cargo.toml")?;
    let Some(target) = lookup_table_path(&doc, table) else {
        return Ok(None);
    };
    let Some(deps_table) = target.get_ref().as_table() else {
        return Ok(None);
    };
    let Some((_, dep_val)) = find_entry(deps_table, name) else {
        return Ok(None);
    };
    let DeValue::Table(subtable) = dep_val.get_ref() else {
        return Ok(None);
    };
    if !parse_workspace_flag(subtable, name)? {
        return Ok(None);
    }

    let resolved_ws = lookup_workspace_dep_for_detach(manifest_dir, doc.get_ref(), subtable, name)?;
    let retention = match resolved_ws.as_ref() {
        Some(res) => PackageRetention::MatchExpected(res.expected_package.as_deref()),
        None => PackageRetention::KeepMember,
    };
    let sanitized_contents = strip_ignored_member_fields(contents, table, name, retention)?;
    Ok(Some(PreparedDetach { sanitized_contents, resolved_ws }))
}

fn strip_ignored_member_fields(
    contents: &str,
    table: &str,
    name: &str,
    retention: PackageRetention<'_>,
) -> Result<String> {
    let mut current = contents.to_string();
    while let Some(range) = next_ignored_field_range(&current, table, name, retention)? {
        current.replace_range(range, "");
    }
    Ok(current)
}

fn next_ignored_field_range(
    contents: &str,
    table: &str,
    name: &str,
    retention: PackageRetention<'_>,
) -> Result<Option<Range<usize>>> {
    let doc = DeTable::parse(contents).into_diagnostic().wrap_err("parse Cargo.toml")?;
    let Some(target) = lookup_table_path(&doc, table) else {
        return Ok(None);
    };
    let target_span = target.span();
    let Some(deps_table) = target.get_ref().as_table() else {
        return Ok(None);
    };
    let Some((_, dep_val)) = find_entry(deps_table, name) else {
        return Ok(None);
    };
    let dep_span = dep_val.span();
    let DeValue::Table(subtable) = dep_val.get_ref() else {
        return Ok(None);
    };
    let Some(entry_span) = find_ignored_entry_span(contents, subtable, name, retention)? else {
        return Ok(None);
    };
    Ok(Some(member_entry_removal_range(
        contents,
        (&target_span, deps_table),
        (&dep_span, subtable),
        entry_span,
    )))
}

fn find_ignored_entry_span(
    contents: &str,
    subtable: &DeTable<'_>,
    name: &str,
    retention: PackageRetention<'_>,
) -> Result<Option<Range<usize>>> {
    let Some((field_key, field_val)) = subtable
        .iter()
        .find(|(key, val)| !keep_member_field(key.get_ref().as_ref(), val, retention))
    else {
        return Ok(None);
    };
    let field_span = field_val.span();
    if matches!(field_val.get_ref(), DeValue::Table(_)) && !is_inline_table(contents, &field_span) {
        let field_name = field_key.get_ref();
        bail!(
            "cannot detach crate {name}: unsupported `{field_name}` subtable on workspace dependency",
        );
    }
    Ok(Some(field_key.span().start..field_span.end))
}

fn keep_member_field(
    key: &str,
    val: &Spanned<DeValue<'_>>,
    retention: PackageRetention<'_>,
) -> bool {
    match key {
        "workspace" | "version" | "features" | "default-features" | "default_features"
        | "optional" | "public" => true,
        "package" => match retention {
            PackageRetention::KeepMember => true,
            PackageRetention::MatchExpected(expected) => val.get_ref().as_str() == expected,
        },
        _ => false,
    }
}

fn member_entry_removal_range(
    contents: &str,
    (target_span, deps_table): (&Range<usize>, &DeTable<'_>),
    (dep_span, subtable): (&Range<usize>, &DeTable<'_>),
    entry_span: Range<usize>,
) -> Range<usize> {
    if is_inline_table(contents, dep_span) {
        inline_entry_removal_range(contents, dep_span, subtable, entry_span)
    } else if is_inline_table(contents, target_span) {
        inline_entry_removal_range(contents, target_span, deps_table, entry_span)
    } else {
        full_line_range(contents, entry_span)
    }
}
