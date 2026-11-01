//! Platform variants' own snapshots (`<name>@<version>/variants/<tag>`):
//! the installer walks the selected variant's dependencies instead of the
//! version's.

use std::collections::HashMap;

use pnpm_lockfile::{
    Lockfile, LockfileResolution, PackageKey, PlatformSelector, SelectedVariant, SnapshotEntry,
};

const VARIANT_SEGMENT: &str = "/variants/";

/// `lockfile` with each package whose selected archive is a variant with a
/// snapshot of its own wired as that snapshot, and without the variants'
/// snapshot keys, so every later phase reads one ordinary snapshot per
/// package. `None` when the lockfile has no variant snapshot.
#[must_use]
pub fn with_selected_variant_snapshots(
    lockfile: &Lockfile,
    platform: &PlatformSelector,
) -> Option<Lockfile> {
    if !has_variant_snapshots(lockfile) {
        return None;
    }
    let mut lockfile = lockfile.clone();
    for (key, entry) in take_variant_snapshots(&mut lockfile) {
        let Some((base, tag)) = split_variant_key(&key) else { continue };
        if selected_tag(&lockfile, &base, platform).as_deref() != Some(tag.as_str()) {
            continue;
        }
        if let Some(snapshot) = lockfile.snapshots
            .as_mut()
            .and_then(|map| map.get_mut(&base))
        {
            snapshot.dependencies = entry.dependencies;
            snapshot.optional_dependencies = entry.optional_dependencies;
            snapshot.transitive_peer_dependencies = entry.transitive_peer_dependencies;
        }
    }
    Some(lockfile)
}

/// Remove and return the variants' own snapshots.
fn take_variant_snapshots(lockfile: &mut Lockfile) -> Vec<(PackageKey, SnapshotEntry)> {
    let Some(snapshots) = lockfile.snapshots.as_mut() else { return Vec::new() };
    let (variants, rest): (HashMap<_, _>, HashMap<_, _>) =
        std::mem::take(snapshots).into_iter().partition(|(key, _)| is_variant_key(key));
    *snapshots = rest;
    variants.into_iter().collect()
}

/// The tag of the variant `platform` selects for the package of `base`, or
/// `None` when it selects the primary or nothing.
fn selected_tag(
    lockfile: &Lockfile,
    base: &PackageKey,
    platform: &PlatformSelector,
) -> Option<String> {
    let metadata = lockfile.packages
        .as_ref()?
        .get(&base.without_peer())?;
    let LockfileResolution::Variations(variations) = &metadata.resolution else { return None };
    match variations.select(platform).ok()?? {
        SelectedVariant::Variant(variant) => variant.variant_tag(),
        SelectedVariant::Fallback(_) => None,
    }
}

/// Whether `lockfile` has a platform variant's own snapshot, so installing
/// it has to go through [`with_selected_variant_snapshots`].
#[must_use]
pub fn has_variant_snapshots(lockfile: &Lockfile) -> bool {
    lockfile.snapshots
        .as_ref()
        .is_some_and(|snapshots| snapshots.keys().any(is_variant_key))
}

fn is_variant_key(key: &PackageKey) -> bool {
    key.to_string().contains(VARIANT_SEGMENT)
}

/// `<name>@<version>/variants/<tag>(<peers>)` as the version's own key,
/// `<name>@<version>(<peers>)`, and the tag.
fn split_variant_key(key: &PackageKey) -> Option<(PackageKey, String)> {
    let key = key.to_string();
    let start = key.find(VARIANT_SEGMENT)?;
    let rest = &key[start + VARIANT_SEGMENT.len()..];
    let end = rest.find('(').unwrap_or(rest.len());
    let base = format!("{}{}", &key[..start], &rest[end..]);
    Some((base.parse().ok()?, rest[..end].to_string()))
}

#[cfg(test)]
mod tests;
