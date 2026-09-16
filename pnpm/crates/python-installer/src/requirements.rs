use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_python_resolver::parse_requirement;
use std::{collections::BTreeSet, path::Path};

pub(super) fn read(path: &Path) -> Result<Vec<String>> {
    let mut requirements = Vec::new();
    read_file(path, &mut BTreeSet::new(), &mut requirements)?;
    Ok(requirements)
}

fn read_file(
    path: &Path,
    visiting: &mut BTreeSet<std::path::PathBuf>,
    requirements: &mut Vec<String>,
) -> Result<()> {
    let canonical = std::fs::canonicalize(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", path.display()))?;
    if !visiting.insert(canonical.clone()) {
        bail!("cyclic Python requirements include: {}", path.display());
    }
    let contents = std::fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", path.display()))?;
    read_entries(path, &contents, visiting, requirements)?;
    visiting.remove(&canonical);
    Ok(())
}

fn read_entries(
    path: &Path,
    contents: &str,
    visiting: &mut BTreeSet<std::path::PathBuf>,
    requirements: &mut Vec<String>,
) -> Result<()> {
    let mut logical = String::new();
    for (index, line) in contents.lines().enumerate() {
        let line = line.trim();
        if let Some(line) = line.strip_suffix('\\') {
            logical.push_str(line);
            continue;
        }
        logical.push_str(line);
        let entry = logical.trim();
        let entry = entry
            .char_indices()
            .find(|(index, character)| {
                *character == '#' && (*index == 0 || entry[..*index].ends_with(char::is_whitespace))
            })
            .map_or(entry, |(index, _)| &entry[..index])
            .trim();
        read_entry(path, index + 1, entry, visiting, requirements)?;
        logical.clear();
    }
    if !logical.is_empty() {
        bail!("unfinished Python requirement continuation in {}", path.display());
    }
    Ok(())
}

fn read_entry(
    path: &Path,
    line: usize,
    entry: &str,
    visiting: &mut BTreeSet<std::path::PathBuf>,
    requirements: &mut Vec<String>,
) -> Result<()> {
    if entry.is_empty() {
        return Ok(());
    }
    let context = || format!("{}:{line}", path.display());
    if let Some(include) = entry
        .strip_prefix("-r")
        .or_else(|| entry.strip_prefix("--requirement "))
        .or_else(|| entry.strip_prefix("--requirement="))
    {
        let include = include.trim();
        if include.is_empty() || include.contains("://") {
            bail!("unsupported Python requirements include at {}: {entry}", context());
        }
        let included = path
            .parent()
            .expect("requirements file has a parent")
            .join(include);
        read_file(&included, visiting, requirements).wrap_err_with(context)?;
    } else {
        if entry.starts_with('-') || entry.contains(" --") {
            bail!("unsupported Python requirements directive at {}: {entry}", context());
        }
        parse_requirement(entry).wrap_err_with(context)?;
        requirements.push(entry.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
