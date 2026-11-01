//! Platform variants' own snapshots (`<name>@<version>/variants/<tag>`):
//! the installer walks the selected variant's dependencies instead of the
//! version's.

use pnpm_lockfile::{Lockfile, LockfileResolution, PackageKey, PlatformSelector, SelectedVariant};

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
    let snapshots = lockfile.snapshots.as_mut()?;
    let variant_keys: Vec<PackageKey> = snapshots
        .keys()
        .filter(|key| is_variant_key(key))
        .cloned()
        .collect();
    let mut variants = Vec::with_capacity(variant_keys.len());
    for key in variant_keys {
        if let Some(entry) = snapshots.remove(&key) {
            variants.push((key, entry));
        }
    }
    for (key, entry) in variants {
        let Some((base, tag)) = split_variant_key(&key) else { continue };
        let selected = lockfile.packages
            .as_ref()
            .and_then(|packages| packages.get(&base.without_peer()))
            .and_then(|metadata| match &metadata.resolution {
                LockfileResolution::Variations(variations) => variations
                    .select(platform)
                    .ok()
                    .flatten(),
                _ => None,
            });
        let is_selected = matches!(
            selected,
            Some(SelectedVariant::Variant(variant)) if variant.variant_tag().as_deref() == Some(&tag)
        );
        if is_selected && let Some(snapshot) = snapshots.get_mut(&base) {
            snapshot.dependencies = entry.dependencies;
            snapshot.optional_dependencies = entry.optional_dependencies;
            snapshot.transitive_peer_dependencies = entry.transitive_peer_dependencies;
        }
    }
    Some(lockfile)
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
