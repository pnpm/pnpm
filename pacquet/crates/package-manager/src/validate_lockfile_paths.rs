//! Reject a lockfile whose virtual-store slots would escape the store
//! root once materialized.
//!
//! Dependency-name validation lives in the lockfile verifier
//! ([`pacquet_lockfile_verification::verify_lockfile_dependency_names`]),
//! which the install runs unconditionally. This module covers the one
//! escape that name validation alone can't: the global-virtual-store
//! slot path inserts the package name and the version-derived segment as
//! raw path components (unlike the legacy flat name, which `/`-escapes),
//! so a traversal in the version escapes the store even when the name is
//! valid. The check needs the install-time [`VirtualStoreLayout`], which
//! is why it lives here rather than in the verifier crate.

use crate::VirtualStoreLayout;
use pacquet_fs::is_subdir;
use pacquet_lockfile::{PackageKey, SnapshotEntry};
use pacquet_lockfile_verification::VerifyError;
use std::collections::{BTreeSet, HashMap};

/// Reject the install when any snapshot's computed virtual-store slot
/// resolves outside the store root. The whole `snapshots` map is
/// scanned — not just the survivors of the warm-install skip filter —
/// so a poisoned snapshot that would be skipped as unchanged is still
/// rejected before any directory is created.
///
/// Surfaces [`VerifyError::InvalidDependencyAlias`]
/// (`ERR_PNPM_INVALID_DEPENDENCY_NAME`), the same code the name check
/// raises, listing every offending snapshot key.
pub fn validate_virtual_store_slot_containment(
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    layout: &VirtualStoreLayout,
) -> Result<(), VerifyError> {
    let Some(snapshots) = snapshots else {
        return Ok(());
    };
    let mut escaped: BTreeSet<String> = BTreeSet::new();
    for key in snapshots.keys() {
        // Lexical containment: the slot does not exist yet, so this must
        // not touch the filesystem.
        if !is_subdir(layout.package_store_dir(), &layout.slot_dir(key)) {
            escaped.insert(key.to_string());
        }
    }
    if escaped.is_empty() {
        return Ok(());
    }
    let escaped: Vec<String> = escaped.into_iter().collect();
    Err(VerifyError::invalid_dependency_aliases(&escaped))
}

#[cfg(test)]
mod tests;
