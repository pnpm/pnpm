use super::Manifest;
use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
    bail,
};
use pep508_rs::Requirement;
use pnpm_python_resolver::parse_requirement;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::Path,
};

pub(crate) fn add(
    path: &Path,
    requirements: &[String],
    development: bool,
    config: &pnpm_config::Config,
) -> Result<()> {
    let original = std::fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", path.display()))?;
    let parsed = Manifest::parse(&original)?;
    let Some(project) = &parsed.project else {
        bail!("{} has no [project] table", path.display());
    };
    project.ensure_static_dependencies()?;
    let updated = with_added_requirements(&original, requirements, development)?;
    let manifest = Manifest::parse(&updated)?;
    if development
        && !manifest
            .group_requirements(config)?
            .iter()
            .any(|(group, _)| group.as_ref() == "dev")
    {
        bail!(
            "cannot add a development dependency to {}: select the dev group in python.groups or [tool.pnpm.python].groups",
            path.display(),
        );
    }
    pnpm_fs::write_atomic(path, updated.as_bytes()).into_diagnostic()
}

fn with_added_requirements(
    original: &str,
    requirements: &[String],
    development: bool,
) -> Result<String> {
    let document: BTreeMap<String, TomlTable> = toml::from_str(original).into_diagnostic()?;
    let (table_name, key) =
        if development { ("dependency-groups", "dev") } else { ("project", "dependencies") };
    let table = document.get(table_name);
    let existing_array = table
        .map(toml::Spanned::get_ref)
        .and_then(|table| {
            if development {
                table
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("dev"))
                    .map(|(_, value)| value)
            } else {
                table.get(key)
            }
        });
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
    let mut updated = original.to_owned();
    if let Some(existing) = existing_array {
        updated.replace_range(existing.span(), &array);
    } else if let Some(table) = table {
        insert_key_into_table(&mut updated, original, table, (table_name, key), &array)?;
    } else {
        writeln!(updated, "\n[{table_name}]\n{key} = {array}").expect(
            "writing to a String cannot fail",
        );
    }
    Ok(updated)
}

/// One table of the manifest, with the span it occupies in the source.
type TomlTable = toml::Spanned<BTreeMap<String, toml::Spanned<toml::Value>>>;

/// Add each requirement to `entries`, replacing an entry that already
/// names the same package for the same environment marker rather than
/// declaring it twice.
fn merge_requirements(entries: &mut Vec<toml::Value>, requirements: &[String]) -> Result<()> {
    for requirement in requirements {
        let parsed = parse_requirement(requirement)?;
        let existing = entries
            .iter()
            .position(|entry| {
                entry
                    .as_str()
                    .map(str::parse::<Requirement>)
                    .and_then(Result::ok)
                    .is_some_and(|entry| entry.name == parsed.name && entry.marker == parsed.marker)
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
        let end = original[span.end..]
            .find('\n')
            .map(|end| span.end + end + 1)
            .unwrap_or(original.len());
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
