//! Configuration, matching, and application logic for pnpm's
//! `patchedDependencies`.
//!
//! The crate exposes:
//!
//! 1. Types, parser, grouping, matcher, verify, hashing, and the
//!    workspace-dir-anchored [`resolve_and_group`] helper for going
//!    from `pnpm-workspace.yaml`'s `patchedDependencies` map to a
//!    [`PatchGroupRecord`].
//! 2. [`get_patch_info()`] for looking up the matching patch for a
//!    `(name, version)` pair (exact → unique range → wildcard) with
//!    `ERR_PNPM_PATCH_KEY_CONFLICT` on ambiguity.
//! 3. [`apply_patch_to_dir`] for applying a unified-diff patch
//!    against an extracted package directory before postinstall
//!    hooks run, using the pure-Rust [`diffy`] crate (`git apply`
//!    and `patch` are ruled out for cross-platform reasons).
//! 4. [`verify_patches`] for the `ERR_PNPM_UNUSED_PATCH` diagnostic
//!    when configured patches don't match any installed dep.
//!
//! pnpm v11 reads `patchedDependencies` from `pnpm-workspace.yaml`,
//! not from `package.json`'s `pnpm` field. [`resolve_and_group`]
//! accordingly takes a workspace dir and a pre-parsed
//! [`IndexMap`][indexmap::IndexMap] — the caller is responsible for
//! surfacing the map (today: from yaml; in the lockfile-only path,
//! from `pnpm-lock.yaml`'s top-level `patchedDependencies` field).

mod apply;
mod get_patch_info;
mod group;
mod hash;
mod key;
mod resolve;
mod types;
mod verify;

pub use apply::{PatchApplyError, apply_patch_to_dir};
pub use get_patch_info::{PatchKeyConflictError, get_patch_info};
pub use group::{PatchInput, PatchNonSemverRangeError, group_patched_dependencies};
pub use hash::{CalcPatchHashError, calc_patch_hashes, create_hex_hash_from_file};
pub use key::{ParsedKey, parse_key};
pub use resolve::{ResolvePatchedDependenciesError, resolve_and_group};
pub use types::{ExtendedPatchInfo, PatchGroup, PatchGroupRangeItem, PatchGroupRecord, PatchInfo};
pub use verify::{UnusedPatchError, UnusedPatches, all_patch_keys, verify_patches};
