use crate::types::PatchGroupRecord;
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::collections::HashSet;

/// Iterate every configured patch key in a stable order.
///
/// Ports upstream's
/// [`allPatchKeys`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/src/allPatchKeys.ts).
pub fn all_patch_keys(patched_dependencies: &PatchGroupRecord) -> impl Iterator<Item = &str> + '_ {
    patched_dependencies.values().flat_map(|group| {
        group
            .exact
            .values()
            .map(|info| info.key.as_str())
            .chain(group.range.iter().map(|item| item.patch.key.as_str()))
            .chain(group.all.iter().map(|info| info.key.as_str()))
    })
}

/// Raised when one or more configured patches were never applied
/// because no package matched their key.
///
/// Mirrors upstream's
/// [`ERR_PNPM_UNUSED_PATCH`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/src/verifyPatches.ts#L25-L29).
#[derive(Debug, Display, Error, Diagnostic)]
#[display("The following patches were not used: {}", unused_patches.join(", "))]
#[diagnostic(
    code(ERR_PNPM_UNUSED_PATCH),
    help(
        r#"Either remove them from "patchedDependencies" or update them to match packages in your dependencies."#
    )
)]
pub struct UnusedPatchError {
    pub unused_patches: Vec<String>,
}

/// Result of [`verify_patches`] when `allow_unused_patches` is true:
/// the caller emits a warning instead of failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnusedPatches {
    pub unused_patches: Vec<String>,
}

/// Check that every configured patch was applied at least once.
///
/// Ports upstream's
/// [`verifyPatches`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/src/verifyPatches.ts).
pub fn verify_patches(
    patched_dependencies: &PatchGroupRecord,
    applied_patches: &HashSet<String>,
    allow_unused_patches: bool,
) -> Result<Option<UnusedPatches>, UnusedPatchError> {
    let unused: Vec<String> = all_patch_keys(patched_dependencies)
        .filter(|key| !applied_patches.contains(*key))
        .map(str::to_string)
        .collect();

    if unused.is_empty() {
        return Ok(None);
    }
    if allow_unused_patches {
        return Ok(Some(UnusedPatches { unused_patches: unused }));
    }
    Err(UnusedPatchError { unused_patches: unused })
}

#[cfg(test)]
mod tests;
