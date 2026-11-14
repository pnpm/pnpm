use super::{
    spans::{dotted_prefix, find_entry, line_start, quoted},
    workspace::{rebase_workspace_path, resolve_local_path, resolve_workspace_dep_for_detach},
};
use miette::{Result, bail};
use std::{
    ops::Range,
    path::{Path, PathBuf},
};
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

pub(super) struct ResolvedWorkspaceDep {
    pub(super) member_dir: PathBuf,
    pub(super) workspace_root: PathBuf,
    pub(super) expected_package: Option<String>,
    pub(super) ws_table: Option<toml::Table>,
}

pub(super) fn lookup_workspace_dep_for_detach(
    manifest_dir: Option<&Path>,
    doc: &DeTable<'_>,
    subtable: &DeTable<'_>,
    name: &str,
) -> Result<Option<ResolvedWorkspaceDep>> {
    let Some(dir) = manifest_dir else {
        return Ok(None);
    };
    let configured_ws = extract_package_workspace(dir, doc, name)?;
    let dep_package = extract_member_package(subtable, name)?;
    let (workspace_root, ws_key, ws_val) =
        resolve_workspace_dep_for_detach(dir, configured_ws, name, dep_package)?;
    if ws_val.is_str() {
        let expected_package = (ws_key != name).then_some(ws_key);
        return Ok(Some(ResolvedWorkspaceDep {
            member_dir: dir.to_path_buf(),
            workspace_root,
            expected_package,
            ws_table: None,
        }));
    }
    let ws_table = ws_val
        .as_table()
        .ok_or_else(|| {
            miette::miette!(
                "cannot detach crate {name}: workspace dependency is not a string or table",
            )
        })?;
    let effective_pkg = optional_string_field(ws_table, "package", name)?.unwrap_or(&ws_key);
    let expected_package = (effective_pkg != name).then(|| effective_pkg.to_string());
    Ok(Some(ResolvedWorkspaceDep {
        member_dir: dir.to_path_buf(),
        workspace_root,
        expected_package,
        ws_table: Some(ws_table.clone()),
    }))
}

pub(super) fn build_detached_inheritance(
    resolved: Option<&ResolvedWorkspaceDep>,
    subtable: &DeTable<'_>,
    name: &str,
) -> Result<DetachedInheritance> {
    let Some(resolved) = resolved else {
        return Ok(DetachedInheritance::default());
    };
    let mut out = DetachedInheritance::default();
    if find_entry(subtable, "package").is_none()
        && let Some(pkg) = resolved.expected_package.as_deref()
    {
        out.extra_fields.push(("package", quoted(pkg)));
    }
    let Some(ws_table) = resolved.ws_table.as_ref() else {
        return Ok(out);
    };
    copy_inherited_source(
        (&resolved.member_dir, &resolved.workspace_root),
        ws_table,
        name,
        &mut out,
    )?;
    if disables_default_features(ws_table) && !member_sets_default_features(subtable) {
        out.extra_fields.push(("default-features", "false".to_string()));
    }
    merge_inherited_features(subtable, ws_table, &mut out);
    Ok(out)
}

pub(super) fn parse_workspace_flag(subtable: &DeTable<'_>, name: &str) -> Result<bool> {
    let Some((_, val)) = find_entry(subtable, "workspace") else {
        return Ok(false);
    };
    match val.get_ref() {
        DeValue::Boolean(flag) => Ok(*flag),
        _ => Err(miette::miette!(
            "cannot update crate {name}: `workspace` in Cargo.toml is not a boolean",
        )),
    }
}

fn extract_package_workspace(
    member_dir: &Path,
    doc: &DeTable<'_>,
    name: &str,
) -> Result<Option<PathBuf>> {
    let Some((_, pkg_val)) = find_entry(doc, "package") else {
        return Ok(None);
    };
    let Some(pkg_table) = pkg_val.get_ref().as_table() else {
        return Ok(None);
    };
    let Some((_, ws_val)) = find_entry(pkg_table, "workspace") else {
        return Ok(None);
    };
    let Some(rel) = ws_val.get_ref().as_str() else {
        bail!("cannot detach crate {name}: `package.workspace` in Cargo.toml is not a string");
    };
    Ok(Some(resolve_local_path(&member_dir.join(rel))))
}

fn extract_member_package<'i>(subtable: &'i DeTable<'i>, name: &str) -> Result<Option<&'i str>> {
    let Some((_, val)) = find_entry(subtable, "package") else {
        return Ok(None);
    };
    val.get_ref()
        .as_str()
        .map(Some)
        .ok_or_else(|| {
            miette::miette!("cannot detach crate {name}: `package` in Cargo.toml is not a string")
        })
}

fn copy_inherited_source(
    (member_dir, workspace_root): (&Path, &Path),
    ws_table: &toml::Table,
    name: &str,
    out: &mut DetachedInheritance,
) -> Result<()> {
    if ws_table.contains_key("base") {
        bail!("cannot detach crate {name}: workspace dependency uses `base` path resolution");
    }
    for field in ["registry", "registry-index"] {
        if let Some(val) = optional_string_field(ws_table, field, name)? {
            out.extra_fields.push((field, quoted(val)));
        }
    }
    copy_git_source(ws_table, name, out)?;
    if let Some(raw_path) = optional_string_field(ws_table, "path", name)? {
        let rebased = rebase_workspace_path(workspace_root, member_dir, raw_path);
        out.extra_fields.push(("path", quoted(&rebased)));
    }
    Ok(())
}

fn copy_git_source(
    ws_table: &toml::Table,
    name: &str,
    out: &mut DetachedInheritance,
) -> Result<()> {
    let Some(git_url) = optional_string_field(ws_table, "git", name)? else {
        return Ok(());
    };
    out.extra_fields.push(("git", quoted(git_url)));
    for selector in ["branch", "tag", "rev"] {
        if let Some(val) = optional_string_field(ws_table, selector, name)? {
            out.extra_fields.push((selector, quoted(val)));
        }
    }
    Ok(())
}

fn optional_string_field<'a>(
    ws_table: &'a toml::Table,
    field: &str,
    name: &str,
) -> Result<Option<&'a str>> {
    let Some(val) = ws_table.get(field) else {
        return Ok(None);
    };
    val.as_str()
        .map(Some)
        .ok_or_else(|| {
            miette::miette!(
                "cannot detach crate {name}: `{field}` in `[workspace.dependencies]` is not a string",
            )
        })
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
