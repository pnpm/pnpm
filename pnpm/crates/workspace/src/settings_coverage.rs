use crate::include::{
    IncludeError,
    parse_mapping,
    read_workspace_manifest_document,
};
use std::path::Path;
use yaml_serde::{
    Mapping,
    Value,
};

#[cfg(test)]
mod tests;

/// The key that shapes a workspace rather than configuring it.
const PACKAGES_KEY: &str = "packages";

/// Whether the `pnpm-workspace.yaml` under `root_dir` already sets every
/// setting that the one under `nested_dir` sets, so an install of the outer
/// workspace loses none of them.
///
/// Both manifests are read with their local includes merged in. `packages` is
/// not a setting and is left out. A mapping covers another when it has each of
/// its keys with a covering value, a list when it holds each of its items, and
/// any other value when it is equal.
///
/// Returns `true` when `nested_dir` has no manifest.
pub fn root_settings_cover(root_dir: &Path, nested_dir: &Path) -> Result<bool, IncludeError> {
    let Some(nested) = read_settings(nested_dir)? else {
        return Ok(true);
    };
    let root = read_settings(root_dir)?.unwrap_or_default();
    Ok(nested
        .iter()
        .filter(|(key, _)| key.as_str() != Some(PACKAGES_KEY))
        .all(|(key, value)| root.get(key).is_some_and(|root_value| covers(root_value, value))))
}

fn read_settings(dir: &Path) -> Result<Option<Mapping>, IncludeError> {
    let Some(document) = read_workspace_manifest_document(dir)? else {
        return Ok(None);
    };
    let settings = parse_mapping(&document.text)
        .map_err(|source| IncludeError::ParseRoot { path: document.path, source })?;
    Ok(Some(settings.unwrap_or_default()))
}

fn covers(root: &Value, nested: &Value) -> bool {
    match (root, nested) {
        (Value::Mapping(root), Value::Mapping(nested)) => nested
            .iter()
            .all(|(key, value)| root.get(key).is_some_and(|root_value| covers(root_value, value))),
        (Value::Sequence(root), Value::Sequence(nested)) => nested
            .iter()
            .all(|item| root.contains(item)),
        _ => root == nested,
    }
}
