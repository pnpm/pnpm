use std::{collections::BTreeMap, path::PathBuf};

/// Information about one configured patch.
///
/// `patch_file_path` is `Some` once the `patchedDependencies` entry
/// has been resolved against the manifest directory and the file has
/// been confirmed to exist; it stays optional so the same shape can
/// carry just a hash (e.g. from the lockfile) without a resolved
/// on-disk file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchInfo {
    /// SHA-256 hex digest of the patch file's bytes.
    pub hash: String,
    /// Absolute path to the patch file, when resolvable.
    pub patch_file_path: Option<PathBuf>,
}

/// A [`PatchInfo`] tagged with the raw `patchedDependencies` key it
/// came from.
///
/// The key is preserved verbatim so unused-patch diagnostics can quote
/// the user's exact configuration key (e.g. `lodash@^4.17.21`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtendedPatchInfo {
    pub hash: String,
    pub patch_file_path: Option<PathBuf>,
    pub key: String,
}

/// One (version-range, patch) pair within a [`PatchGroup`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchGroupRangeItem {
    pub version: String,
    pub patch: ExtendedPatchInfo,
}

/// All configured patches for one package name, partitioned by match
/// flavor.
///
/// `exact` uses [`BTreeMap`] for deterministic iteration; `range`
/// preserves the order in which the user listed entries.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PatchGroup {
    pub exact: BTreeMap<String, ExtendedPatchInfo>,
    pub range: Vec<PatchGroupRangeItem>,
    pub all: Option<ExtendedPatchInfo>,
}

/// Resolved `patchedDependencies`, keyed by package name.
///
/// Iteration is alphabetical so error messages and lockfile-stable
/// diagnostics stay deterministic across runs.
pub type PatchGroupRecord = BTreeMap<String, PatchGroup>;
