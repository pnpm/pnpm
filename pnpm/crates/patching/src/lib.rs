//! Reading, applying, and committing the patch files of pnpm's
//! `patchedDependencies`.
//!
//! - [`resolve_and_group`] reads the setting's patch files relative to the
//!   workspace dir and hashes them into a [`PatchGroupRecord`].
//! - [`apply_patch_to_dir`] applies a unified-diff patch to an extracted
//!   package directory before postinstall hooks run.
//! - [`prepare_pkg_files_for_diff`] and [`diff_folders`] create the filtered
//!   package diff written by `pnpm patch-commit`.
//!
//! Parsing, grouping, and matching the entries live in
//! [`pnpm_patching_config`], which this crate re-exports.

pub(crate) use pnpm_process as process;

pub use apply::{
    MANIFEST_FILE_NAME, PatchApplyError, PatchPreview, apply_patch_to_dir, preview_patch,
};
pub use commit::*;
pub use hash::{CalcPatchHashError, calc_patch_hashes, create_hex_hash_from_file};
pub use pnpm_patching_config::{
    ExtendedPatchInfo, ParsedKey, PatchGroup, PatchGroupRangeItem, PatchGroupRecord, PatchInfo,
    PatchInput, PatchKeyConflictError, PatchNonSemverRangeError, UnusedPatchError, UnusedPatches,
    all_patch_keys, get_patch_info, group_patched_dependencies, parse_key, verify_patches,
};
pub use resolve::{ResolvePatchedDependenciesError, resolve_and_group};

mod apply;
mod commit;
mod hash;
mod resolve;
