//! The entry points an archive's manifest names must be files the archive
//! holds.

use super::{BTreeSet, PackError, Value, VariantEntry, required_field_paths};

/// Refuse an archive whose manifest's `main`, `browser`, or `bin` does not
/// name, by its exact path, a file the archive holds (`visible`, by its path
/// in the archive). The packlist adds those files by their exact paths, so
/// a variant names them that way: no extension is added, and a directory
/// is not resolved to its `index` or its own `package.json`.
pub(super) fn check_entry_points(
    entry: &VariantEntry,
    manifest: &Value,
    visible: &BTreeSet<String>,
) -> Result<(), PackError> {
    match required_field_paths(manifest)
        .into_iter()
        .find(|path| !visible.contains(&format!("package/{path}")))
    {
        Some(path) => Err(PackError::VariantEntryPointMissing { targets: entry.label(), path }),
        None => Ok(()),
    }
}
