use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    ops::Range,
    path::Path,
};

use miette::{IntoDiagnostic, Result, WrapErr};
use pep508_rs::{PackageName, Requirement};
use pnpm_python_resolver::parse_requirement;

use crate::{manifest, outdated::PythonDependencyGroup};

type TomlTable = toml::Spanned<BTreeMap<String, toml::Spanned<toml::Value>>>;

pub(super) fn rewrite_pyproject(
    path: &Path,
    mut transform: impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Requirement>>,
) -> Result<()> {
    let original = fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", path.display()))?;
    let non_index = non_index_sources(&original)?;
    let document: BTreeMap<String, TomlTable> = toml::from_str(&original).into_diagnostic()?;
    let mut edits = collect_pyproject_edits(&document, &non_index, &mut transform)?;
    if edits.is_empty() {
        return Ok(());
    }
    edits.sort_by_key(|right| std::cmp::Reverse(right.0.start));
    let mut updated = original;
    for (span, replacement) in edits {
        updated.replace_range(span, &replacement);
    }
    manifest::Manifest::parse(&updated)?;
    pnpm_fs::write_atomic(path, updated.as_bytes()).into_diagnostic()
}

fn non_index_sources(original: &str) -> Result<BTreeSet<PackageName>> {
    let parsed = manifest::Manifest::parse(original)?;
    Ok(parsed.tool.uv.sources
        .iter()
        .filter(|(_, decl)| {
            decl.sources()
                .iter()
                .any(|source| {
                    source.workspace
                        || source.path.is_some()
                        || source.git.is_some()
                        || source.url.is_some()
                })
        })
        .map(|(name, _)| name.clone())
        .collect())
}

fn collect_pyproject_edits(
    document: &BTreeMap<String, TomlTable>,
    non_index: &BTreeSet<PackageName>,
    transform: &mut impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Requirement>>,
) -> Result<Vec<(Range<usize>, String)>> {
    let mut edits = Vec::new();
    if let Some(project) = document.get("project") {
        collect_project_edits(project.get_ref(), non_index, transform, &mut edits)?;
    }
    if let Some(groups) = document.get("dependency-groups") {
        for value in groups.get_ref().values() {
            if let Some(edit) =
                rewrite_spanned_array(value, PythonDependencyGroup::Dev, non_index, transform)?
            {
                edits.push(edit);
            }
        }
    }
    Ok(edits)
}

fn collect_project_edits(
    project: &BTreeMap<String, toml::Spanned<toml::Value>>,
    non_index: &BTreeSet<PackageName>,
    transform: &mut impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Requirement>>,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<()> {
    if let Some(deps) = project.get("dependencies")
        && let Some(edit) =
            rewrite_spanned_array(deps, PythonDependencyGroup::Prod, non_index, transform)?
    {
        edits.push(edit);
    }
    if let Some(opt_deps) = project.get("optional-dependencies")
        && let Some(edit) = rewrite_spanned_table_of_arrays(
            opt_deps,
            PythonDependencyGroup::Optional,
            non_index,
            transform,
        )?
    {
        edits.push(edit);
    }
    Ok(())
}

fn rewrite_spanned_array(
    spanned: &toml::Spanned<toml::Value>,
    group: PythonDependencyGroup,
    non_index: &BTreeSet<PackageName>,
    transform: &mut impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Requirement>>,
) -> Result<Option<(Range<usize>, String)>> {
    let Some(entries) = spanned.get_ref().as_array() else {
        return Ok(None);
    };
    let (new_entries, changed) = rewrite_array_entries(entries, group, non_index, transform)?;
    Ok(changed.then(|| (spanned.span(), toml::Value::Array(new_entries).to_string())))
}

fn rewrite_spanned_table_of_arrays(
    spanned: &toml::Spanned<toml::Value>,
    group: PythonDependencyGroup,
    non_index: &BTreeSet<PackageName>,
    transform: &mut impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Requirement>>,
) -> Result<Option<(Range<usize>, String)>> {
    let Some(table) = spanned.get_ref().as_table() else {
        return Ok(None);
    };
    let mut updated_table = table.clone();
    let mut any_changed = false;
    for (key, val) in table {
        let Some(entries) = val.as_array() else {
            continue;
        };
        let (new_entries, changed) = rewrite_array_entries(entries, group, non_index, transform)?;
        if changed {
            any_changed = true;
            updated_table.insert(key.clone(), toml::Value::Array(new_entries));
        }
    }
    Ok(any_changed.then(|| (spanned.span(), toml::Value::Table(updated_table).to_string())))
}

fn rewrite_array_entries(
    entries: &[toml::Value],
    group: PythonDependencyGroup,
    non_index: &BTreeSet<PackageName>,
    transform: &mut impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Requirement>>,
) -> Result<(Vec<toml::Value>, bool)> {
    let mut out = Vec::with_capacity(entries.len());
    let mut changed = false;
    for entry in entries {
        if let Some(rendered) = rewrite_single_entry(entry, group, non_index, transform)? {
            changed = true;
            out.push(toml::Value::String(rendered));
        } else {
            out.push(entry.clone());
        }
    }
    Ok((out, changed))
}

fn rewrite_single_entry(
    entry: &toml::Value,
    group: PythonDependencyGroup,
    non_index: &BTreeSet<PackageName>,
    transform: &mut impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Requirement>>,
) -> Result<Option<String>> {
    let Some(raw_req) = entry.as_str() else {
        return Ok(None);
    };
    let Ok(req) = parse_requirement(raw_req) else {
        return Ok(None);
    };
    if non_index.contains(&req.name) {
        return Ok(None);
    }
    let Some(updated_req) = transform(group, req)? else {
        return Ok(None);
    };
    let rendered = updated_req.to_string();
    Ok((rendered != raw_req).then_some(rendered))
}
