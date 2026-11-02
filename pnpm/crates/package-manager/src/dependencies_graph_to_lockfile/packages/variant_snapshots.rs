//! A platform variant's own snapshot: the dependencies resolved from the
//! variant's manifest, under `<name>@<version>/variants/<tag>(<peers>)`.
//!
//! The resolver resolves every archive's dependencies as the package's
//! children. The version's snapshot keeps those its primary manifest
//! declares, and each variant whose resolved snapshot differs gets one of
//! its own, so the lockfile is the same on every machine.

use pnpm_lockfile::{PackageKey, PkgName, SnapshotDepRef, SnapshotEntry};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// The dependency names one archive's manifest declares, and which of them
/// it declares optional.
struct Declared {
    names: HashSet<String>,
    optional: HashSet<String>,
}

impl Declared {
    fn of(manifest: &Value) -> Self {
        let keys = |field: &str| -> HashSet<String> {
            manifest
                .get(field)
                .and_then(Value::as_object)
                .map(|map| map.keys().cloned().collect())
                .unwrap_or_default()
        };
        let bundled: HashSet<String> = match ["bundledDependencies", "bundleDependencies"]
            .into_iter()
            .find_map(|field| manifest.get(field))
        {
            Some(Value::Bool(true)) => keys("dependencies"),
            Some(Value::Array(names)) => names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
            _ => HashSet::new(),
        };
        let optional: HashSet<String> = &keys("optionalDependencies") - &bundled;
        let mut names: HashSet<String> = &(&keys("dependencies") | &optional) - &bundled;
        names.extend(keys("peerDependencies"));
        Declared { names, optional }
    }
}

/// Split `full`, the snapshot of every archive's children, into the
/// version's snapshot and the snapshots of the variants that differ from
/// it. `manifest` is the resolved version's manifest; a version without
/// variants keeps `full` as it is.
pub(super) fn split_variant_snapshots(
    snapshot_key: &PackageKey,
    manifest: Option<&Value>,
    full: SnapshotEntry,
) -> (SnapshotEntry, Vec<(PackageKey, SnapshotEntry)>) {
    let Some(manifest) = manifest else { return (full, Vec::new()) };
    let variants = pnpm_platform_variants::variant_manifests(manifest);
    if variants.is_empty() {
        return (full, Vec::new());
    }
    let primary = Declared::of(manifest);
    let declared: Vec<(String, Declared)> = variants
        .iter()
        .map(|(tag, variant)| (tag.clone(), Declared::of(variant)))
        .collect();
    let by_any: HashSet<&String> = declared
        .iter()
        .flat_map(|(_, variant)| &variant.names)
        .chain(&primary.names)
        .collect();
    let base = restrict(&full, &primary, &by_any);
    let variant_snapshots = declared
        .iter()
        .filter_map(|(tag, variant)| {
            let entry = restrict(&full, variant, &by_any);
            (entry != base)
                .then(|| Some((variant_key(snapshot_key, tag)?, entry)))
                .flatten()
        })
        .collect();
    (base, variant_snapshots)
}

/// `full` with only the children `archive` declares, each optional as the
/// archive declares it. A child no archive declares, such as an engines
/// runtime, stays.
fn restrict(full: &SnapshotEntry, archive: &Declared, by_any: &HashSet<&String>) -> SnapshotEntry {
    let mut dependencies: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    let mut optional_dependencies: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    let children = full.dependencies
        .iter()
        .chain(&full.optional_dependencies)
        .flatten();
    for (name, reference) in children {
        let alias = name.to_string();
        if by_any.contains(&alias) && !archive.names.contains(&alias) {
            continue;
        }
        let was_optional = full.optional_dependencies
            .as_ref()
            .is_some_and(|map| map.contains_key(name));
        let optional =
            if by_any.contains(&alias) { archive.optional.contains(&alias) } else { was_optional };
        let target = if optional { &mut optional_dependencies } else { &mut dependencies };
        target.insert(name.clone(), reference.clone());
    }
    SnapshotEntry {
        dependencies: (!dependencies.is_empty()).then_some(dependencies),
        optional_dependencies: (!optional_dependencies.is_empty()).then_some(optional_dependencies),
        ..full.clone()
    }
}

/// `<name>@<version>/variants/<tag>`, with the peer suffix of `key` after it.
fn variant_key(key: &PackageKey, tag: &str) -> Option<PackageKey> {
    let key = key.to_string();
    let at = key[1..].find('@')? + 1;
    let (name_version, peers) = match key[at..].find('(') {
        Some(paren) => key.split_at(at + paren),
        None => (key.as_str(), ""),
    };
    format!("{name_version}/variants/{tag}{peers}").parse().ok()
}

#[cfg(test)]
mod tests;
