//! The `patchedDependencies` setting as data: its keys, how its entries
//! group by package name, which patch applies to a package version, and
//! which entries matched nothing. Reading and applying patch files is in
//! `pnpm-patching`.

pub use get_patch_info::{PatchKeyConflictError, get_patch_info};
pub use group::{PatchInput, PatchNonSemverRangeError, group_patched_dependencies};
pub use key::{ParsedKey, parse_key};
pub use types::{ExtendedPatchInfo, PatchGroup, PatchGroupRangeItem, PatchGroupRecord, PatchInfo};
pub use verify::{UnusedPatchError, UnusedPatches, all_patch_keys, verify_patches};

mod get_patch_info;
mod group;
mod key;
mod types;
mod verify;
