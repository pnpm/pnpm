use miette::{IntoDiagnostic, Result};
use std::{collections::HashSet, fs, path::Path};

pub(super) fn read_patched_crate_names(root_manifest_path: &Path) -> Result<HashSet<String>> {
    if !root_manifest_path.is_file() {
        return Ok(HashSet::new());
    }
    let contents = fs::read_to_string(root_manifest_path).into_diagnostic()?;
    let doc: toml::Table = toml::from_str(&contents).into_diagnostic()?;
    let Some(patches) = doc.get("patch").and_then(toml::Value::as_table) else {
        return Ok(HashSet::new());
    };
    Ok(patches
        .values()
        .filter_map(toml::Value::as_table)
        .flat_map(|crates| {
            crates
                .iter()
                .map(|(name, value)| {
                    value
                        .as_table()
                        .and_then(|patch| patch.get("package"))
                        .and_then(toml::Value::as_str)
                        .unwrap_or(name)
                        .to_string()
                })
        })
        .collect())
}
