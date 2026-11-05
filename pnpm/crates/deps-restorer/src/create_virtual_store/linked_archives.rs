//! Which archive a slot of a package with platform variants holds. A local
//! virtual-store slot's path does not name the archive, so after a change
//! of `supportedArchitectures`, or of the host (a `node_modules` mounted
//! into a musl container, a new Node.js for a variant chosen by its ABI),
//! the slot may hold another archive than the one the install links. Each
//! such slot records, beside the package directory, the integrity of the
//! archive it was imported from; a slot whose record differs from the
//! integrity the lockfile lists for the chosen archive, or that has none,
//! is replaced.

use crate::{VirtualStoreLayout, platform_selection::VariantPlatforms};
use pnpm_lockfile::{LockfileResolution, PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_package_is_installable::SupportedArchitectures;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

/// The record in a slot of the integrity of the archive the slot holds.
const LINKED_ARCHIVE_RECORD: &str = ".pnpm-linked-archive";

/// The integrity the lockfile lists for the archive `supported` links of the
/// package of `snapshot_key`, when it is a registry package with platform
/// variants.
fn linked_integrity(
    snapshot_key: &PackageKey,
    packages: &HashMap<PackageKey, PackageMetadata>,
    supported: Option<&SupportedArchitectures>,
) -> Option<String> {
    let Some(LockfileResolution::Variations(variations)) = packages
        .get(&snapshot_key.without_peer())
        .map(|metadata| &metadata.resolution)
    else {
        return None;
    };
    if variations.is_runtime() {
        return None;
    }
    let platforms = VariantPlatforms::of(supported);
    let selected = variations
        .select(platforms.for_variations(variations))
        .ok()
        .flatten()?;
    selected.resolution.integrity().map(ToString::to_string)
}

fn record_path(layout: &VirtualStoreLayout, snapshot_key: &PackageKey) -> PathBuf {
    layout.slot_dir(snapshot_key).join(LINKED_ARCHIVE_RECORD)
}

fn package_dir(layout: &VirtualStoreLayout, snapshot_key: &PackageKey) -> PathBuf {
    layout
        .slot_dir(snapshot_key)
        .join("node_modules")
        .join(snapshot_key.name.to_string())
}

/// Whether the slot of `snapshot_key` holds another archive of a package with
/// platform variants than the one `supported` links: its record names
/// another integrity, or it has none.
pub(crate) fn slot_holds_other_variant(
    layout: &VirtualStoreLayout,
    (snapshot_key, packages): (&PackageKey, &HashMap<PackageKey, PackageMetadata>),
    supported: Option<&SupportedArchitectures>,
) -> bool {
    let Some(linked) = linked_integrity(snapshot_key, packages, supported) else {
        return false;
    };
    if !package_dir(layout, snapshot_key).is_dir() {
        return false;
    }
    match std::fs::read_to_string(record_path(layout, snapshot_key)) {
        Ok(recorded) => recorded.trim() != linked,
        Err(_) => true,
    }
}

/// Record in each materialized slot of a package with platform variants the
/// integrity of the archive it now holds. Every slot whose record differed
/// was replaced by this install, so the archive it holds is the one
/// `supported` links. A record that cannot be written only makes the next
/// install replace the slot.
pub(crate) fn record_linked_archives(
    layout: &VirtualStoreLayout,
    (snapshots, packages): (
        &HashMap<PackageKey, SnapshotEntry>,
        &HashMap<PackageKey, PackageMetadata>,
    ),
    supported: Option<&SupportedArchitectures>,
    fetch_failed: &HashSet<PackageKey>,
) {
    for snapshot_key in snapshots
        .keys()
        .filter(|key| !fetch_failed.contains(*key))
    {
        let Some(linked) = linked_integrity(snapshot_key, packages, supported) else {
            continue;
        };
        if !package_dir(layout, snapshot_key).is_dir() {
            continue;
        }
        let path = record_path(layout, snapshot_key);
        if std::fs::read_to_string(&path).is_ok_and(|recorded| recorded.trim() == linked) {
            continue;
        }
        if let Err(error) = std::fs::write(&path, &linked) {
            tracing::debug!(path = %path.display(), %error, "could not record the linked archive");
        }
    }
}
