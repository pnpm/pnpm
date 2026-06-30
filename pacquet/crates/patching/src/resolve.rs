use crate::{
    group::{PatchInput, PatchNonSemverRangeError, group_patched_dependencies},
    hash::{CalcPatchHashError, create_hex_hash_from_file},
    types::PatchGroupRecord,
};
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use std::path::{Path, PathBuf};

/// Error resolving `patchedDependencies` against a workspace dir.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolvePatchedDependenciesError {
    #[diagnostic(transparent)]
    Hash(#[error(source)] CalcPatchHashError),

    #[diagnostic(transparent)]
    Range(#[error(source)] PatchNonSemverRangeError),
}

impl From<CalcPatchHashError> for ResolvePatchedDependenciesError {
    fn from(error: CalcPatchHashError) -> Self {
        Self::Hash(error)
    }
}

impl From<PatchNonSemverRangeError> for ResolvePatchedDependenciesError {
    fn from(error: PatchNonSemverRangeError) -> Self {
        Self::Range(error)
    }
}

/// Resolve relative patch file paths, hash each file, and bucket the
/// resulting entries with [`group_patched_dependencies`].
///
/// `raw` is the `patchedDependencies` map as it appears in
/// `pnpm-workspace.yaml`: keys are `name[@version]` (e.g.
/// `lodash@4.17.21`) and values are patch file paths, either
/// relative to `workspace_dir` or absolute. The map's iteration
/// order is preserved end-to-end into [`PatchGroup::range`], so
/// the order in `PATCH_KEY_CONFLICT` diagnostics matches the order
/// the user wrote in yaml. [`IndexMap`] is required for that; a
/// [`BTreeMap`] would sort the keys and reorder ranges
/// alphabetically.
///
/// Each raw path is resolved against `workspace_dir`, hashed into a
/// [`PatchInput`] entry, and then grouped. pnpm v11 reads install
/// settings (including `patchedDependencies`) from the *workspace*
/// manifest, not from `package.json`'s `pnpm` field.
///
/// [`BTreeMap`]: std::collections::BTreeMap
/// [`PatchGroup::range`]: crate::PatchGroup::range
pub fn resolve_and_group(
    workspace_dir: &Path,
    raw: &IndexMap<String, String>,
) -> Result<Option<PatchGroupRecord>, ResolvePatchedDependenciesError> {
    if raw.is_empty() {
        return Ok(None);
    }

    let mut inputs: Vec<(String, PatchInput)> = Vec::with_capacity(raw.len());
    for (key, rel_or_abs) in raw {
        let candidate = Path::new(rel_or_abs);
        let resolved: PathBuf = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            workspace_dir.join(candidate)
        };
        let hash = create_hex_hash_from_file(&resolved)?;
        inputs.push((key.clone(), PatchInput { hash, patch_file_path: Some(resolved) }));
    }

    let groups = group_patched_dependencies(inputs)?;
    Ok(Some(groups))
}

#[cfg(test)]
mod tests;
