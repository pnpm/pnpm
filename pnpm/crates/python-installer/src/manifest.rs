use miette::{IntoDiagnostic, Result, bail};
use pep508_rs::Requirement;
use pnpm_config::Config;
use pnpm_python_resolver::parse_requirement;
use serde::Deserialize;
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

#[derive(Clone, Copy)]
pub struct DependencySelection {
    pub production: bool,
    pub development: bool,
}

impl DependencySelection {
    pub const ALL: Self = Self { production: true, development: true };
}

#[derive(Deserialize)]
pub(super) struct Manifest {
    pub(super) project: Option<Project>,
    #[serde(default, rename = "dependency-groups")]
    groups: BTreeMap<String, Vec<toml::Value>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct Project {
    #[serde(default)]
    pub(super) dependencies: Vec<String>,
    #[serde(default)]
    dynamic: Vec<String>,
    pub(super) requires_python: Option<String>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, Vec<String>>,
}

impl Project {
    fn ensure_static_dependencies(&self) -> Result<()> {
        if self.dynamic.iter().any(|field| {
            matches!(field.as_str(), "dependencies" | "optional-dependencies" | "requires-python")
        }) {
            bail!("pnpm Python integration requires static dependency metadata in pyproject.toml");
        }
        Ok(())
    }
}

impl Manifest {
    pub(super) fn parse(contents: &str) -> Result<Self> {
        toml::from_str(contents).into_diagnostic()
    }

    pub(super) fn requirements(
        &self,
        config: &Config,
        selection: DependencySelection,
    ) -> Result<Vec<Requirement>> {
        let Some(project) = &self.project else { return Ok(Vec::new()) };
        project.ensure_static_dependencies()?;
        let mut requirements =
            if selection.production { project.dependencies.clone() } else { Vec::new() };
        for extra in config.python.extras.iter().filter(|_| selection.production) {
            let dependencies = project
                .optional_dependencies
                .get(extra)
                .ok_or_else(|| miette::miette!("unknown Python project extra: {extra}"))?;
            requirements.extend(dependencies.iter().cloned());
        }
        if selection.development {
            self.expand_configured_groups(config, &mut requirements)?;
        }
        requirements.into_iter().map(|requirement| parse_requirement(&requirement)).collect()
    }

    /// Expand every dependency group the config asks for. The `dev` group
    /// is asked for by default, so a project that declares no such group
    /// simply has none.
    fn expand_configured_groups(
        &self,
        config: &Config,
        requirements: &mut Vec<String>,
    ) -> Result<()> {
        for group in &config.python.groups {
            if group == "dev" && !self.groups.contains_key(group) {
                continue;
            }
            self.expand_group(group, &mut Vec::new(), requirements)?;
        }
        Ok(())
    }

    fn expand_group(
        &self,
        group: &str,
        visiting: &mut Vec<String>,
        requirements: &mut Vec<String>,
    ) -> Result<()> {
        if visiting.iter().any(|name| name == group) {
            bail!("cyclic Python dependency group: {group}");
        }
        let entries = self
            .groups
            .get(group)
            .ok_or_else(|| miette::miette!("unknown Python dependency group: {group}"))?;
        visiting.push(group.to_string());
        for entry in entries {
            if let Some(requirement) = entry.as_str() {
                requirements.push(requirement.to_string());
            } else if let Some(table) = entry.as_table()
                && table.len() == 1
                && let Some(include) = table.get("include-group").and_then(toml::Value::as_str)
            {
                self.expand_group(include, visiting, requirements)?;
            } else {
                bail!("invalid entry in Python dependency group {group}");
            }
        }
        visiting.pop();
        Ok(())
    }
}

pub(crate) fn add(path: &Path, requirements: &[String], development: bool) -> Result<()> {
    let original = std::fs::read_to_string(path).into_diagnostic()?;
    let parsed = Manifest::parse(&original)?;
    let Some(project) = &parsed.project else {
        bail!("{} has no [project] table", path.display());
    };
    project.ensure_static_dependencies()?;
    let document: BTreeMap<String, TomlTable> = toml::from_str(&original).into_diagnostic()?;
    let (table_name, key) =
        if development { ("dependency-groups", "dev") } else { ("project", "dependencies") };
    let table = document.get(table_name);
    let existing_array = table.and_then(|table| table.get_ref().get(key));
    let mut entries = existing_array
        .map(|array| {
            array
                .get_ref()
                .as_array()
                .cloned()
                .ok_or_else(|| miette::miette!("Python dependencies must be an array"))
        })
        .transpose()?
        .unwrap_or_default();
    merge_requirements(&mut entries, requirements)?;
    let array = toml::Value::Array(entries).to_string();
    let mut updated = original.clone();
    if let Some(existing) = existing_array {
        updated.replace_range(existing.span(), &array);
    } else if let Some(table) = table {
        insert_key_into_table(&mut updated, &original, table, (table_name, key), &array)?;
    } else {
        writeln!(updated, "\n[{table_name}]\n{key} = {array}")
            .expect("writing to a String cannot fail");
    }
    Manifest::parse(&updated)?;
    pnpm_fs::write_atomic(path, updated.as_bytes()).into_diagnostic()
}

/// One table of the manifest, with the span it occupies in the source.
type TomlTable = toml::Spanned<BTreeMap<String, toml::Spanned<toml::Value>>>;

/// Add each requirement to `entries`, replacing an entry that already
/// names the same package rather than declaring it twice.
fn merge_requirements(entries: &mut Vec<toml::Value>, requirements: &[String]) -> Result<()> {
    for requirement in requirements {
        let parsed = parse_requirement(requirement)?;
        let existing = entries.iter().position(|entry| {
            entry
                .as_str()
                .and_then(|value| value.parse::<Requirement>().ok())
                .is_some_and(|entry| entry.name == parsed.name)
        });
        if let Some(index) = existing {
            entries[index] = toml::Value::String(requirement.clone());
        } else {
            entries.push(toml::Value::String(requirement.clone()));
        }
    }
    Ok(())
}

/// Write `key = <array>` into a table that does not declare it yet,
/// following the table's own TOML representation.
fn insert_key_into_table(
    updated: &mut String,
    original: &str,
    table: &TomlTable,
    (table_name, key): (&str, &str),
    array: &str,
) -> Result<()> {
    let span = table.span();
    if original[span.clone()].trim_start().starts_with('[') {
        let end = original[span.end..].find('\n').map_or(original.len(), |end| span.end + end + 1);
        updated.insert_str(end, &format!("\n{key} = {array}\n"));
        return Ok(());
    }
    if !original[span.clone()].trim_start().starts_with('{') {
        bail!("cannot add {table_name}.{key} to this TOML table representation");
    }
    let separator = if table.get_ref().is_empty() { "" } else { "," };
    updated.insert_str(span.end - 1, &format!("{separator} {key} = {array}"));
    Ok(())
}
