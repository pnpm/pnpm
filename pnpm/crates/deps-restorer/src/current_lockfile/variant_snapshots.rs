//! Platform variants' own snapshots (`<name>@<version>/variants/<tag>`):
//! the installer walks the selected variant's dependencies instead of the
//! version's.

use std::collections::HashMap;

use pnpm_lockfile::{
    Lockfile, LockfileResolution, PackageKey, PlatformSelector, Prefix, SnapshotEntry, VersionPart,
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

/// Each version's variants' own snapshot keys, by the version's own key.
pub(super) fn variant_keys_by_base(lockfile: &Lockfile) -> HashMap<PackageKey, Vec<PackageKey>> {
    let mut by_base: HashMap<PackageKey, Vec<PackageKey>> = HashMap::new();
    for key in lockfile.snapshots.iter().flat_map(HashMap::keys) {
        if let Some((base, _)) = split_variant_key(key) {
            by_base
                .entry(base)
                .or_default()
                .push(key.clone());
        }
    }
    by_base
}

/// The key of the snapshot a variant tagged `tag` of the version of `base`
/// has of its own, `<name>@<version>/variants/<tag>(<peers>)`.
pub(super) fn variant_snapshot_key(base: &PackageKey, tag: &str) -> Option<PackageKey> {
    let base = base.to_string();
    let end = base.find('(').unwrap_or(base.len());
    format!("{}{VARIANT_SEGMENT}{tag}{}", &base[..end], &base[end..]).parse().ok()
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
/// `None` when it selects the variant that targets `any`, or nothing.
fn selected_tag(
    lockfile: &Lockfile,
    base: &PackageKey,
    platform: &PlatformSelector,
) -> Option<String> {
    let metadata = lockfile.packages
        .as_ref()?
        .get(&base.without_peer())?;
    let LockfileResolution::Variations(variations) = &metadata.resolution else { return None };
    variations
        .select(platform)
        .ok()??
        .variant_tag()
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
    split_variant_key(key).is_some()
}

/// `<name>@<version>/variants/<tag>(<peers>)` as the version's own key,
/// `<name>@<version>(<peers>)`, and the tag. Only a registry version has
/// variants, so the version's own key must name a semver version, from the
/// default registry or a named one: a `file:` path, a URL, or a runtime
/// that happens to hold `/variants/` is no variant key.
fn split_variant_key(key: &PackageKey) -> Option<(PackageKey, String)> {
    let key = key.to_string();
    let start = key.find(VARIANT_SEGMENT)?;
    // The variant part comes before any peer suffix: a `/variants/` inside
    // one belongs to a peer's version.
    if key[..start].contains('(') {
        return None;
    }
    let rest = &key[start + VARIANT_SEGMENT.len()..];
    let end = rest.find('(').unwrap_or(rest.len());
    let tag = &rest[..end];
    if tag.is_empty() || tag.contains('/') {
        return None;
    }
    let base: PackageKey = format!("{}{}", &key[..start], &rest[end..]).parse().ok()?;
    let registry_version = matches!(
        base.suffix.version(),
        VersionPart::Semver(_) | VersionPart::RegistryQualified { .. },
    );
    (registry_version && base.suffix.prefix() == Prefix::None).then(|| (base, tag.to_string()))
}

#[cfg(test)]
mod tests;
