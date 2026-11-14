use super::{
    spans::{dotted_prefix, find_entry, line_start, quoted},
    workspace_dependency_table,
};
use std::{ops::Range, path::Path};
use toml::{
    Spanned,
    de::{DeTable, DeValue},
};

#[derive(Clone, Copy)]
pub(super) enum DetachedShape<'a> {
    Inline,
    DottedInInline(&'a str),
    Subtable,
    Dotted,
}

#[derive(Default)]
pub(super) struct DetachedInheritance {
    pub(super) extra_fields: Vec<(&'static str, String)>,
    pub(super) merged_features_edit: Option<(Range<usize>, String)>,
}

pub(super) fn resolve_detached_inheritance(
    manifest_dir: Option<&Path>,
    subtable: &DeTable<'_>,
    name: &str,
) -> DetachedInheritance {
    let is_inherited = find_entry(subtable, "workspace")
        .is_some_and(|(_, val)| matches!(val.get_ref(), DeValue::Boolean(true)));
    let Some(dir) = manifest_dir.filter(|_| is_inherited) else {
        return DetachedInheritance::default();
    };
    let dep_package = find_entry(subtable, "package").and_then(|(_, val)| val.get_ref().as_str());
    let Some(ws_table) = workspace_dependency_table(dir, name, dep_package) else {
        return DetachedInheritance::default();
    };

    let mut out = DetachedInheritance::default();
    if find_entry(subtable, "package").is_none()
        && let Some(pkg) = ws_table.get("package").and_then(toml::Value::as_str)
        && pkg != name
    {
        out.extra_fields.push(("package", quoted(pkg)));
    }
    if disables_default_features(&ws_table) && !member_sets_default_features(subtable) {
        out.extra_fields.push(("default-features", "false".to_string()));
    }
    merge_inherited_features(subtable, &ws_table, &mut out);
    out
}

fn disables_default_features(ws_table: &toml::Table) -> bool {
    ws_table
        .get("default-features")
        .or_else(|| ws_table.get("default_features"))
        .and_then(toml::Value::as_bool)
        == Some(false)
}

fn member_sets_default_features(subtable: &DeTable<'_>) -> bool {
    find_entry(subtable, "default-features").is_some()
        || find_entry(subtable, "default_features").is_some()
}

fn merge_inherited_features(
    subtable: &DeTable<'_>,
    ws_table: &toml::Table,
    out: &mut DetachedInheritance,
) {
    let ws_features: Vec<&str> = ws_table
        .get("features")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .collect();
    if ws_features.is_empty() {
        return;
    }
    let Some((_, member_val)) = find_entry(subtable, "features") else {
        let deduped = dedup_features(ws_features);
        out.extra_fields.push(("features", format_string_array(&deduped)));
        return;
    };
    let Some(member_features) = extract_string_array(member_val) else {
        return;
    };
    if ws_features
        .iter()
        .all(|feat| member_features.contains(feat))
    {
        return;
    }
    let merged = dedup_features(ws_features.into_iter().chain(member_features));
    out.merged_features_edit = Some((member_val.span(), format_string_array(&merged)));
}

fn extract_string_array<'i>(value: &'i Spanned<DeValue<'i>>) -> Option<Vec<&'i str>> {
    let DeValue::Array(items) = value.get_ref() else {
        return None;
    };
    items
        .iter()
        .map(|item| item.get_ref().as_str())
        .collect()
}

fn dedup_features<'a>(iter: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut out = Vec::new();
    for feat in iter {
        if !out.contains(&feat) {
            out.push(feat);
        }
    }
    out
}

fn format_string_array(items: &[&str]) -> String {
    let formatted: Vec<String> = items
        .iter()
        .map(|item| quoted(item))
        .collect();
    format!("[{}]", formatted.join(", "))
}

pub(super) fn format_detached_replacement(
    contents: &str,
    span: &Range<usize>,
    ws_key_start: usize,
    shape: DetachedShape<'_>,
    version_spec: Option<&str>,
    extra_fields: &[(&str, String)],
) -> String {
    let mut entries = Vec::with_capacity(extra_fields.len() + usize::from(version_spec.is_some()));
    if let Some(spec) = version_spec {
        entries.push(("version", quoted(spec)));
    }
    entries.extend(
        extra_fields
            .iter()
            .map(|(key, val)| (*key, val.clone())),
    );

    let sep = match shape {
        DetachedShape::Inline => ", ".to_string(),
        DetachedShape::DottedInInline(raw_key) => format!(", {raw_key}."),
        DetachedShape::Subtable => {
            let indent = &contents[line_start(contents, ws_key_start)..ws_key_start];
            format!("\n{indent}")
        }
        DetachedShape::Dotted => {
            let prefix = dotted_prefix(contents, span);
            let indent = leading_indent(contents, ws_key_start);
            format!("\n{indent}{prefix}.")
        }
    };
    entries
        .into_iter()
        .map(|(key, val)| format!("{key} = {val}"))
        .collect::<Vec<_>>()
        .join(&sep)
}

fn leading_indent(contents: &str, pos: usize) -> &str {
    let start = line_start(contents, pos);
    let len = contents[start..pos]
        .bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count();
    &contents[start..start + len]
}
