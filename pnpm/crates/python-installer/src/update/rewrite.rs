use std::{
    collections::BTreeSet,
    fs,
    ops::Range,
    path::Path,
};

use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
};
use pep508_rs::{
    PackageName,
    Requirement,
};
use pnpm_python_resolver::parse_requirement;
use toml::{
    Spanned,
    de::{
        DeTable,
        DeValue,
    },
};

use crate::{
    manifest,
    outdated::PythonDependencyGroup,
    workspace::Workspace,
};

#[cfg(test)]
mod tests;

pub(super) fn rewrite_pyproject(
    member: &Path,
    workspace: &Workspace,
    mut transform: impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Requirement>>,
) -> Result<()> {
    rewrite_pyproject_many(member, workspace, |group, req| {
        Ok(transform(group, req)?.map(|updated| vec![updated]))
    })
}

pub(super) fn rewrite_pyproject_many(
    member: &Path,
    workspace: &Workspace,
    mut transform: impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Vec<Requirement>>>,
) -> Result<()> {
    let path = member.join("pyproject.toml");
    let original = match fs::read_to_string(&path) {
        Ok(original) => original,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && member.join("requirements.txt").is_file() =>
        {
            return Ok(());
        }
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", path.display()));
        }
    };
    let parsed = manifest::Manifest::parse(&original)?;
    let non_index = workspace.non_index_sources(member, &parsed);
    let Some(updated) = rewrite_manifest_text(&original, &non_index, &mut transform)? else {
        return Ok(());
    };
    pnpm_fs::write_atomic(&path, updated.as_bytes()).into_diagnostic()
}

pub(crate) fn remove_requirements(
    path: &Path,
    names: &BTreeSet<PackageName>,
    group: Option<PythonDependencyGroup>,
    removed: &mut BTreeSet<PackageName>,
) -> Result<()> {
    let original = fs::read_to_string(path).into_diagnostic()?;
    if let Some(updated) = rewrite_manifest_text(&original, &BTreeSet::new(), &mut |field, req| {
        if group.is_none_or(|group| group == field) && names.contains(&req.name) {
            removed.insert(req.name);
            Ok(Some(Vec::new()))
        } else {
            Ok(None)
        }
    })? {
        pnpm_fs::write_atomic(path, updated.as_bytes()).into_diagnostic()?;
    }
    Ok(())
}

fn rewrite_manifest_text(
    original: &str,
    non_index: &BTreeSet<PackageName>,
    transform: &mut impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Vec<Requirement>>>,
) -> Result<Option<String>> {
    let document = DeTable::parse(original).into_diagnostic()?;
    let mut edits = collect_pyproject_edits(document.get_ref(), original, non_index, transform)?;
    if edits.is_empty() {
        return Ok(None);
    }
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0.start));
    let mut updated = original.to_owned();
    for (span, replacement) in edits {
        updated.replace_range(span, &replacement);
    }
    manifest::Manifest::parse(&updated)?;
    Ok(Some(updated))
}

fn collect_pyproject_edits(
    document: &DeTable<'_>,
    original: &str,
    non_index: &BTreeSet<PackageName>,
    transform: &mut impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Vec<Requirement>>>,
) -> Result<Vec<(Range<usize>, String)>> {
    let mut edits = Vec::new();
    if let Some(project) =
        table_entry(document, "project").map(Spanned::get_ref).and_then(DeValue::as_table)
    {
        collect_project_edits(project, original, non_index, transform, &mut edits)?;
    }
    if let Some(groups) = table_entry(document, "dependency-groups") {
        collect_table_of_arrays_edits(
            groups,
            original,
            PythonDependencyGroup::Dev,
            non_index,
            transform,
            &mut edits,
        )?;
    }
    Ok(edits)
}

fn collect_project_edits(
    project: &DeTable<'_>,
    original: &str,
    non_index: &BTreeSet<PackageName>,
    transform: &mut impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Vec<Requirement>>>,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<()> {
    if let Some(deps) = table_entry(project, "dependencies") {
        collect_array_edits(
            deps,
            original,
            PythonDependencyGroup::Prod,
            non_index,
            transform,
            edits,
        )?;
    }
    if let Some(opt_deps) = table_entry(project, "optional-dependencies") {
        collect_table_of_arrays_edits(
            opt_deps,
            original,
            PythonDependencyGroup::Optional,
            non_index,
            transform,
            edits,
        )?;
    }
    Ok(())
}

fn collect_table_of_arrays_edits(
    spanned: &Spanned<DeValue<'_>>,
    original: &str,
    group: PythonDependencyGroup,
    non_index: &BTreeSet<PackageName>,
    transform: &mut impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Vec<Requirement>>>,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<()> {
    let Some(table) = spanned.get_ref().as_table() else {
        return Ok(());
    };
    for value in table.values() {
        collect_array_edits(value, original, group, non_index, transform, edits)?;
    }
    Ok(())
}

fn collect_array_edits(
    spanned: &Spanned<DeValue<'_>>,
    original: &str,
    group: PythonDependencyGroup,
    non_index: &BTreeSet<PackageName>,
    transform: &mut impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Vec<Requirement>>>,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<()> {
    let DeValue::Array(entries) = spanned.get_ref() else {
        return Ok(());
    };
    let outcomes = entries
        .iter()
        .map(|entry| rewrite_single_entry(entry.get_ref(), group, non_index, transform))
        .collect::<Result<Vec<_>>>()?;
    if outcomes.iter().all(Option::is_none) {
        return Ok(());
    }
    if outcomes
        .iter()
        .map(Option::as_ref)
        .any(|value| value.is_some_and(Vec::is_empty))
    {
        edits.push((spanned.span(), render_filtered_array(entries, &outcomes, original)));
        return Ok(());
    }
    for (entry, rendered) in entries
        .iter()
        .zip(outcomes)
        .filter_map(|(entry, outcome)| outcome.map(|rendered| (entry, rendered)))
    {
        edits.push((entry.span(), format_rendered_entries(&rendered)));
    }
    Ok(())
}

fn render_filtered_array(
    entries: &[Spanned<DeValue<'_>>],
    outcomes: &[Option<Vec<String>>],
    original: &str,
) -> String {
    let mut kept = Vec::with_capacity(entries.len());
    for (entry, outcome) in entries.iter().zip(outcomes) {
        match outcome {
            None => kept.push(original[entry.span()].to_owned()),
            Some(rendered) => kept.extend(
                rendered
                    .iter()
                    .cloned()
                    .map(format_toml_string),
            ),
        }
    }
    format!("[{}]", kept.join(", "))
}

fn format_rendered_entries(rendered: &[String]) -> String {
    rendered
        .iter()
        .cloned()
        .map(format_toml_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_toml_string(value: String) -> String {
    toml::Value::String(value).to_string()
}

fn table_entry<'a, 'i>(table: &'a DeTable<'i>, name: &str) -> Option<&'a Spanned<DeValue<'i>>> {
    table
        .iter()
        .find_map(|(key, value)| (key.get_ref().as_ref() == name).then_some(value))
}

fn rewrite_single_entry(
    entry: &DeValue<'_>,
    group: PythonDependencyGroup,
    non_index: &BTreeSet<PackageName>,
    transform: &mut impl FnMut(PythonDependencyGroup, Requirement) -> Result<Option<Vec<Requirement>>>,
) -> Result<Option<Vec<String>>> {
    let Some(raw_req) = entry.as_str() else {
        return Ok(None);
    };
    let Ok(req) = parse_requirement(raw_req) else {
        return Ok(None);
    };
    if non_index.contains(&req.name) {
        return Ok(None);
    }
    let Some(updated_reqs) = transform(group, req)? else {
        return Ok(None);
    };
    let rendered: Vec<String> = updated_reqs
        .into_iter()
        .map(|item| item.to_string())
        .collect();
    if rendered.len() == 1 && rendered[0] == raw_req {
        return Ok(None);
    }
    Ok(Some(rendered))
}
