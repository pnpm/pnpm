pub(crate) mod workspace;

pub(crate) use workspace::{
    ConfiguredWorkspace, configured_workspace_state, lookup_inherited_workspace_entry,
    resolve_local_path, workspace_dependency_spec, workspace_includes_member,
};

mod inheritance;
mod sanitize;
mod spans;

use inheritance::{
    DetachedShape, ResolvedWorkspaceDep, build_detached_inheritance, format_detached_replacement,
};
use miette::{IntoDiagnostic, Result, WrapErr};
use sanitize::prepare_workspace_detach;
use spans::{
    append_new_table, apply_edits, collect_header_spans, dotted_prefix, dotted_table_end,
    end_of_line, find_entry, format_key, full_line_range, inline_entry_removal_range,
    insert_into_inline_table, insert_line_at, is_explicit_header, is_inline_table,
    lookup_table_path, quoted, replace_range, section_end,
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
    let manifest_dir = manifest_path.parent();
    let mut updated = original.clone();
    for (name, version_spec) in dependencies {
        updated =
            upsert_dependency_in_dir(&updated, manifest_dir, kind.table(), name, version_spec)?;
    }
    if updated != original {
        pnpm_fs::write_atomic(manifest_path, updated.as_bytes())
            .into_diagnostic()
            .wrap_err_with(|| format!("write {}", manifest_path.display()))?;
    }
    Ok(())
}

pub(crate) fn upsert_dependency_in_dir(
    contents: &str,
    manifest_dir: Option<&Path>,
    table: &str,
    name: &str,
    version_spec: &str,
) -> Result<String> {
    let prepared = prepare_workspace_detach(contents, manifest_dir, table, name)?;
    let (working, resolved_ws) = match prepared.as_ref() {
        Some(prep) => (prep.sanitized_contents.as_str(), prep.resolved_ws.as_ref()),
        None => (contents, None),
    };
    upsert_sanitized_dependency(working, resolved_ws, table, name, version_spec)
}

fn upsert_sanitized_dependency(
    contents: &str,
    resolved_ws: Option<&ResolvedWorkspaceDep>,
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
    let target_span = target.span();

    if let Some((dep_key, dep_val)) = find_entry(deps_table, name) {
        let inline_parent = is_inline_table(contents, &target_span)
            .then_some(InlineParent {
                span: &target_span,
                table: deps_table,
                key_text: &contents[dep_key.span()],
            });
        let ctx =
            UpdateContext { contents, resolved_ws, header_spans: &header_spans, inline_parent };
        return update_existing_dependency(&ctx, dep_val, name, version_spec);
    }

    Ok(insert_new_dependency(
        contents,
        &header_spans,
        (&target_span, deps_table),
        table,
        (name, version_spec),
    ))
}

fn insert_new_dependency(
    contents: &str,
    header_spans: &[Range<usize>],
    (target_span, deps_table): (&Range<usize>, &DeTable<'_>),
    table: &str,
    (name, version_spec): (&str, &str),
) -> String {
    let entry = format!("{} = {}", format_key(name), quoted(version_spec));
    if is_explicit_header(contents, target_span) {
        let end = section_end(contents, header_spans, target_span);
        return insert_line_at(contents, end, &entry);
    }
    if is_inline_table(contents, target_span) {
        return insert_into_inline_table(contents, target_span, deps_table, &entry);
    }
    if header_spans
        .iter()
        .any(|header| header.start <= target_span.start && target_span.end <= header.end)
    {
        return append_new_table(contents, table, name, version_spec);
    }
    let prefix = dotted_prefix(contents, target_span);
    let after =
        end_of_line(contents, dotted_table_end(contents, deps_table, header_spans, target_span));
    insert_line_at(contents, after, &format!("{prefix}.{entry}"))
}

#[derive(Clone, Copy)]
struct InlineParent<'a, 'i> {
    span: &'a Range<usize>,
    table: &'a DeTable<'i>,
    key_text: &'a str,
}

#[derive(Clone, Copy)]
struct UpdateContext<'a, 'i> {
    contents: &'a str,
    resolved_ws: Option<&'a ResolvedWorkspaceDep>,
    header_spans: &'a [Range<usize>],
    inline_parent: Option<InlineParent<'a, 'i>>,
}

#[derive(Clone, Copy)]
enum TableShape<'a, 'i> {
    Inline,
    DottedInInline(InlineParent<'a, 'i>),
    Subtable(&'a [Range<usize>]),
    Dotted(&'a [Range<usize>]),
}

impl<'a> TableShape<'a, '_> {
    fn detached(self) -> DetachedShape<'a> {
        match self {
            Self::Inline => DetachedShape::Inline,
            Self::DottedInInline(parent) => DetachedShape::DottedInInline(parent.key_text),
            Self::Subtable(_) => DetachedShape::Subtable,
            Self::Dotted(_) => DetachedShape::Dotted,
        }
    }
}

fn update_existing_dependency(
    ctx: &UpdateContext<'_, '_>,
    dep_val: &Spanned<DeValue<'_>>,
    name: &str,
    version_spec: &str,
) -> Result<String> {
    match dep_val.get_ref() {
        DeValue::String(_) => {
            Ok(replace_range(ctx.contents, dep_val.span(), &quoted(version_spec)))
        }
        DeValue::Table(subtable) => {
            let span = dep_val.span();
            let shape = if is_inline_table(ctx.contents, &span) {
                TableShape::Inline
            } else if is_explicit_header(ctx.contents, &span) {
                TableShape::Subtable(ctx.header_spans)
            } else if let Some(parent) = ctx.inline_parent {
                TableShape::DottedInInline(parent)
            } else {
                TableShape::Dotted(ctx.header_spans)
            };
            update_table_dependency(ctx, &span, subtable, shape, (name, version_spec))
        }
        _ => Err(miette::miette!(
            "cannot update crate {name}: its Cargo.toml dependency declaration is not a string or table"
        )),
    }
}

fn update_table_dependency(
    ctx: &UpdateContext<'_, '_>,
    span: &Range<usize>,
    subtable: &DeTable<'_>,
    shape: TableShape<'_, '_>,
    (name, version_spec): (&str, &str),
) -> Result<String> {
    match (find_entry(subtable, "version"), find_entry(subtable, "workspace")) {
        (Some((_, ver_val)), None) => {
            ensure_string_version(ver_val, name)?;
            Ok(replace_range(ctx.contents, ver_val.span(), &quoted(version_spec)))
        }
        (None, Some((ws_key, ws_val))) => replace_workspace_entry(
            ctx,
            span,
            subtable,
            shape,
            ws_key.span().start..ws_val.span().end,
            (name, version_spec),
        ),
        (Some((_, ver_val)), Some((ws_key, ws_val))) => {
            ensure_string_version(ver_val, name)?;
            replace_version_and_workspace(
                ctx,
                span,
                subtable,
                shape,
                (ver_val.span(), ws_key.span().start..ws_val.span().end),
                (name, version_spec),
            )
        }
        (None, None) => {
            Ok(insert_missing_version(ctx.contents, span, subtable, shape, version_spec))
        }
    }
}

fn replace_workspace_entry(
    ctx: &UpdateContext<'_, '_>,
    span: &Range<usize>,
    subtable: &DeTable<'_>,
    shape: TableShape<'_, '_>,
    ws_span: Range<usize>,
    (name, version_spec): (&str, &str),
) -> Result<String> {
    let detached = build_detached_inheritance(ctx.resolved_ws, subtable, name)?;
    let replacement = format_detached_replacement(
        ctx.contents,
        span,
        ws_span.start,
        shape.detached(),
        Some(version_spec),
        &detached.extra_fields,
    );
    let mut edits = vec![(ws_span, replacement)];
    if let Some(feat_edit) = detached.merged_features_edit {
        edits.push(feat_edit);
    }
    Ok(apply_edits(ctx.contents, edits))
}

fn replace_version_and_workspace(
    ctx: &UpdateContext<'_, '_>,
    span: &Range<usize>,
    subtable: &DeTable<'_>,
    shape: TableShape<'_, '_>,
    (ver_span, ws_span): (Range<usize>, Range<usize>),
    (name, version_spec): (&str, &str),
) -> Result<String> {
    let detached = build_detached_inheritance(ctx.resolved_ws, subtable, name)?;
    let ws_edit = if detached.extra_fields.is_empty() {
        (workspace_removal_range(ctx.contents, span, subtable, shape, ws_span), String::new())
    } else {
        let replacement = format_detached_replacement(
            ctx.contents,
            span,
            ws_span.start,
            shape.detached(),
            None,
            &detached.extra_fields,
        );
        (ws_span, replacement)
    };
    let mut edits = vec![(ver_span, quoted(version_spec)), ws_edit];
    if let Some(feat_edit) = detached.merged_features_edit {
        edits.push(feat_edit);
    }
    Ok(apply_edits(ctx.contents, edits))
}

fn workspace_removal_range(
    contents: &str,
    span: &Range<usize>,
    subtable: &DeTable<'_>,
    shape: TableShape<'_, '_>,
    ws_span: Range<usize>,
) -> Range<usize> {
    match shape {
        TableShape::Inline => inline_entry_removal_range(contents, span, subtable, ws_span),
        TableShape::DottedInInline(parent) => {
            inline_entry_removal_range(contents, parent.span, parent.table, ws_span)
        }
        TableShape::Subtable(_) | TableShape::Dotted(_) => full_line_range(contents, ws_span),
    }
}

fn insert_missing_version(
    contents: &str,
    span: &Range<usize>,
    subtable: &DeTable<'_>,
    shape: TableShape<'_, '_>,
    version_spec: &str,
) -> String {
    let version_entry = format!("version = {}", quoted(version_spec));
    match shape {
        TableShape::Inline => insert_into_inline_table(contents, span, subtable, &version_entry),
        TableShape::DottedInInline(parent) => insert_into_inline_table(
            contents,
            parent.span,
            parent.table,
            &format!("{}.{version_entry}", parent.key_text),
        ),
        TableShape::Subtable(header_spans) => {
            insert_line_at(contents, section_end(contents, header_spans, span), &version_entry)
        }
        TableShape::Dotted(header_spans) => {
            let prefix = dotted_prefix(contents, span);
            let after =
                end_of_line(contents, dotted_table_end(contents, subtable, header_spans, span));
            insert_line_at(contents, after, &format!("{prefix}.{version_entry}"))
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

#[cfg(test)]
mod tests;
