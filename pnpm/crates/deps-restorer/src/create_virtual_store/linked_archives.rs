//! Which archive the installed copy of a package with platform variants
//! holds. Neither a local virtual-store slot's path nor a hoisted package's
//! directory names the archive, so after a change of
//! `supportedArchitectures`, or of the host (a `node_modules` mounted into a
//! musl container, a new Node.js for a variant chosen by its ABI), the copy
//! may hold another archive than the one the install links. Each such copy
//! records the integrity of the archive it was imported from: a slot beside
//! its package directory, a hoisted package inside it. A copy whose record
//! differs from the integrity the lockfile lists for the chosen archive, or
//! that has none, is imported again.

use crate::{VirtualStoreLayout, platform_selection::VariantPlatforms};
use pnpm_lockfile::{LockfileResolution, PackageKey, PackageMetadata};
use pnpm_package_is_installable::SupportedArchitectures;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// The file that records the integrity of the archive a copy holds.
pub(crate) const LINKED_ARCHIVE_RECORD: &str = ".pnpm-linked-archive";

/// The integrity the lockfile lists for the archive `supported` links of a
/// registry package with platform variants resolved as `resolution`.
pub(crate) fn linked_integrity(
    resolution: &LockfileResolution,
    supported: Option<&SupportedArchitectures>,
) -> Option<String> {
    let LockfileResolution::Variations(variations) = resolution else {
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

/// Whether the record at `record` names `linked`.
pub(crate) fn record_names(record: &Path, linked: &str) -> bool {
    std::fs::read_to_string(record).is_ok_and(|recorded| recorded.trim() == linked)
}

/// Write `linked` to the record at `record`, replacing the file rather than
/// writing through it: a package that ships a file of the record's name has
/// it hard-linked from the store, which other installs share. A record that
/// cannot be written only makes the next install import the copy again.
pub(crate) fn write_record(record: &Path, linked: &str) {
    if let Err(error) = pnpm_fs::write_atomic(record, linked.as_bytes()) {
        tracing::debug!(path = %record.display(), %error, "could not record the linked archive");
    }
}

fn snapshot_linked_integrity(
    snapshot_key: &PackageKey,
    packages: &HashMap<PackageKey, PackageMetadata>,
    supported: Option<&SupportedArchitectures>,
) -> Option<String> {
    let metadata = packages.get(&snapshot_key.without_peer())?;
    linked_integrity(&metadata.resolution, supported)
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
    let Some(linked) = snapshot_linked_integrity(snapshot_key, packages, supported) else {
        return false;
    };
    package_dir(layout, snapshot_key).is_dir()
        && !record_names(&record_path(layout, snapshot_key), &linked)
}

/// Record in each slot this pass imported, of a package with platform
/// variants, the integrity of the archive `supported` links, which is the
/// one the slot now holds. A slot the plan kept already records it, and a
/// snapshot the install skips is not imported, so neither is written.
pub(crate) fn record_linked_archives(
    layout: &VirtualStoreLayout,
    (imported, packages): (&[PackageKey], &HashMap<PackageKey, PackageMetadata>),
    supported: Option<&SupportedArchitectures>,
    fetch_failed: &HashSet<PackageKey>,
) {
    for snapshot_key in imported
        .iter()
        .filter(|key| !fetch_failed.contains(*key))
    {
        let Some(linked) = snapshot_linked_integrity(snapshot_key, packages, supported) else {
            continue;
        };
        if package_dir(layout, snapshot_key).is_dir() {
            write_record(&record_path(layout, snapshot_key), &linked);
        }
    }
}

/// Import again each present hoisted package with platform variants whose
/// directory records another archive than the one `supported` links, or
/// none: the lockfile records the package unchanged when only the setting or
/// the host changed.
pub(crate) fn import_other_hoisted_archives(
    graph: &mut crate::DependenciesGraph,
    supported: Option<&SupportedArchitectures>,
) {
    for node in graph.values_mut().filter(|node| node.present) {
        if let Some(linked) = linked_integrity(&node.package.resolution, supported)
            && !record_names(&node.dir.join(LINKED_ARCHIVE_RECORD), &linked)
        {
            node.present = false;
        }
    }
}

/// Record in each hoisted package directory this pass imported, of a package
/// with platform variants, the integrity of the archive the install links. A
/// present directory already records it, and one whose files were not
/// fetched was not imported.
pub(crate) fn record_hoisted_archives(
    graph: &crate::DependenciesGraph,
    files_by_pkg_id: Option<&crate::CasPathsByPkgId>,
    supported: Option<&SupportedArchitectures>,
) {
    let Some(files_by_pkg_id) = files_by_pkg_id else { return };
    let imported = graph
        .values()
        .filter(|node| {
            !node.present
                && files_by_pkg_id.contains_key(&node.package.pkg_id_with_patch_hash)
                && node.dir.is_dir()
        });
    for node in imported {
        if let Some(linked) = linked_integrity(&node.package.resolution, supported) {
            write_record(&node.dir.join(LINKED_ARCHIVE_RECORD), &linked);
        }
    }
}

#[cfg(test)]
mod tests;
