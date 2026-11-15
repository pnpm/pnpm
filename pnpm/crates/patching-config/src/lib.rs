//! The `patchedDependencies` setting as data: its keys, the hashes of the
//! patch files it names, how its entries group by package name, which patch
//! applies to a package version, and which entries matched nothing. Applying
//! and committing patch files is in `pnpm-patching`.

pub use get_patch_info::{PatchKeyConflictError, get_patch_info};
pub use group::{PatchInput, PatchNonSemverRangeError, group_patched_dependencies};
pub use hash::{CalcPatchHashError, calc_patch_hashes, create_hex_hash_from_file};
pub use key::{ParsedKey, parse_key};
pub use resolve::{ResolvePatchedDependenciesError, resolve_and_group};
pub use types::{ExtendedPatchInfo, PatchGroup, PatchGroupRangeItem, PatchGroupRecord, PatchInfo};
pub use verify::{UnusedPatchError, UnusedPatches, all_patch_keys, verify_patches};

mod get_patch_info;
mod group;
mod hash;
mod key;
mod resolve;
mod types;
mod verify;
