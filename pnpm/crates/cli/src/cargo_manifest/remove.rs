mod features;

use super::{
    spans::{
        apply_edits,
        full_line_range,
        is_explicit_header,
        is_inline_table,
    },
    workspace_dependency_spec,
};
use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
};
use pnpm_package_manifest::DependencyGroup;
use std::{
    collections::BTreeSet,
    ops::Range,
    path::Path,
};
use toml::{
    Spanned,
    de::{
        DeTable,
        DeValue,
    },
};

pub(crate) fn remove_dependencies(
    path: &Path,
    names: &BTreeSet<String>,
    group: Option<DependencyGroup>,
    removed: &mut BTreeSet<String>,
) -> Result<()> {
    let original = std::fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", path.display()))?;
    let mut manifest: toml::Table = toml::from_str(&original).into_diagnostic()?;
    let mut paths = Vec::new();
    let mut selection = Removal { path, names, group, removed };
    selection.remove_from(&mut manifest, &[], &mut paths);
    if let Some(targets) = manifest.get_mut("target").and_then(toml::Value::as_table_mut) {
        for (target, value) in targets.iter_mut() {
            if let Some(table) = value.as_table_mut() {
                selection.remove_from(table, &["target", target], &mut paths);
            }
        }
    }
    if paths.is_empty() {
        return Ok(());
    }
    features::prune_feature_references(&mut manifest, &mut paths);
    let updated = render_removal(original, &manifest, paths, path)?;
    pnpm_fs::write_atomic(path, updated.as_bytes()).into_diagnostic()
}

fn render_removal(
    original: String,
    manifest: &toml::Table,
    paths: Vec<Vec<String>>,
    path: &Path,
) -> Result<String> {
    let source = DeTable::parse(&original).into_diagnostic()?;
    let mut edits = Vec::new();
    for path in paths {
        declaration_edits(&original, source.get_ref(), manifest, &path, &mut edits);
    }
    edits.sort_by_key(|edit| edit.0.start);
    edits.dedup_by(|left, right| left.0 == right.0);
    let updated = apply_edits(original, edits);
    let checked: toml::Table = toml::from_str(&updated).into_diagnostic()?;
    if !same_declarations(manifest, &checked) {
        miette::bail!("could not preserve the other declarations in {}", path.display());
    }
    Ok(updated)
}

// Removing the last dotted declaration can also remove its implicit table.
fn same_declarations(expected: &toml::Table, actual: &toml::Table) -> bool {
    actual
        .keys()
        .all(|key| expected.contains_key(key))
        && expected
            .iter()
            .all(|(key, value)| match (value, actual.get(key)) {
                (toml::Value::Table(table), Some(toml::Value::Table(other))) => {
                    same_declarations(table, other)
                }
                (toml::Value::Table(table), None) => same_declarations(table, &toml::Table::new()),
                (_, other) => other == Some(value),
            })
}

struct Removal<'a> {
    path: &'a Path,
    names: &'a BTreeSet<String>,
    group: Option<DependencyGroup>,
    removed: &'a mut BTreeSet<String>,
}

impl Removal<'_> {
    fn remove_from(
        &mut self,
        parent: &mut toml::Table,
        prefix: &[&str],
        paths: &mut Vec<Vec<String>>,
    ) {
        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            let Some(entries) = parent.get_mut(section).and_then(toml::Value::as_table_mut) else {
                continue;
            };
            entries.retain(|alias, value| {
                if !self.selects(alias, value, section) {
                    return true;
                }
                paths.push(
                    prefix
                        .iter()
                        .copied()
                        .chain([section, alias])
                        .map(str::to_owned)
                        .collect(),
                );
                false
            });
        }
    }

    fn selects(&mut self, alias: &str, value: &toml::Value, section: &str) -> bool {
        let flag = |key: &str| {
            value
                .get(key)
                .and_then(toml::Value::as_bool)
                .unwrap_or(false)
        };
        let group = if flag("optional") {
            DependencyGroup::Optional
        } else if section == "dependencies" {
            DependencyGroup::Prod
        } else {
            DependencyGroup::Dev
        };
        if self.group.is_some_and(|selected| selected != group) {
            return false;
        }
        let package = value.get("package").and_then(toml::Value::as_str);
        let inherited = flag("workspace")
            .then(|| workspace_dependency_spec(self.path.parent()?, alias, package))
            .flatten();
        let name = inherited
            .as_ref()
            .map(|spec| spec.package_name.as_str())
            .or(package)
            .unwrap_or(alias);
        let matched = self.names.contains(alias) || self.names.contains(name);
        if matched {
            self.removed.extend(
                [alias, name]
                    .into_iter()
                    .filter(|id| self.names.contains(*id))
                    .map(str::to_owned),
            );
        }
        matched
    }
}

fn declaration_edits(
    text: &str,
    source: &DeTable<'_>,
    remaining: &toml::Table,
    path: &[String],
    edits: &mut Vec<(Range<usize>, String)>,
) {
    let (name, rest) = path.split_first().expect("dependency path");
    let (key, value) =
        super::spans::find_entry(source, name).expect("source of dependency declaration");
    if rest.is_empty() {
        if let Some(replacement) = remaining.get(name) {
            edits.push((value.span(), replacement.to_string()));
        } else {
            remove_declaration(text, key.span(), value, edits);
        }
    } else if is_inline_table(text, &value.span()) {
        edits.push((value.span(), remaining[name].to_string()));
    } else {
        declaration_edits(
            text,
            value
                .get_ref()
                .as_table()
                .expect("dependency table"),
            remaining[name].as_table().expect("dependency table"),
            rest,
            edits,
        );
    }
}

fn remove_declaration(
    text: &str,
    key: Range<usize>,
    value: &Spanned<DeValue<'_>>,
    edits: &mut Vec<(Range<usize>, String)>,
) {
    let span = value.span();
    if let Some(table) = value.get_ref().as_table()
        && !is_inline_table(text, &span)
    {
        if is_explicit_header(text, &span) {
            edits.push((full_line_range(text, span), String::new()));
        }
        for (child, item) in table {
            remove_declaration(text, child.span(), item, edits);
        }
    } else {
        edits.push((full_line_range(text, key.start..span.end), String::new()));
    }
}

#[cfg(test)]
mod tests;
