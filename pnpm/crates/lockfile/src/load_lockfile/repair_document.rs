//! Narrowing a lockfile document to the parts a repairing install can
//! keep.
//!
//! `--fix-lockfile` regenerates the fields a resolution derives, so a
//! document whose generated fields no longer decode is still usable: the
//! entries that do decode name the packages and pin the versions the
//! repair must not change. Each helper below drops only what it cannot
//! read, so the repair starts from as much of the broken file as the
//! types allow.

use crate::{
    Lockfile,
    LockfileResolution,
    SnapshotEntry,
};

pub(super) fn prepare_value_for_fix(value: &mut serde_json::Value) {
    let Some(root) = value.as_object_mut() else { return };
    for key in [
        "settings",
        "catalogs",
        "overrides",
        "packageExtensionsChecksum",
        "pnpmfileChecksum",
        "ignoredOptionalDependencies",
        "patchedDependencies",
        "time",
    ] {
        discard_invalid_generated_field(root, key);
    }
    if let Some(packages) = root.get_mut("packages").and_then(serde_json::Value::as_object_mut) {
        packages.retain(|_, metadata| reduce_package_metadata(metadata));
    }
    if let Some(snapshots) = root.get_mut("snapshots").and_then(serde_json::Value::as_object_mut) {
        for snapshot in snapshots.values_mut() {
            reduce_snapshot(snapshot);
        }
    }
}

/// Keep a `packages:` entry that already decodes, or narrow it to the
/// resolution alone. An entry with no decodable resolution names nothing and
/// is dropped.
fn reduce_package_metadata(metadata: &mut serde_json::Value) -> bool {
    if serde_json::from_value::<super::super::PackageMetadata>(metadata.clone()).is_ok() {
        return true;
    }
    let Some(resolution) = metadata.get("resolution").cloned() else { return false };
    if serde_json::from_value::<LockfileResolution>(resolution.clone()).is_err() {
        return false;
    }
    *metadata = serde_json::json!({ "resolution": resolution });
    true
}

/// Keep a `snapshots:` entry that already decodes, or narrow it to its
/// dependency edges. An entry whose edges do not decode either is emptied
/// rather than dropped: the key still names a package the graph reaches.
fn reduce_snapshot(snapshot: &mut serde_json::Value) {
    if serde_json::from_value::<SnapshotEntry>(snapshot.clone()).is_ok() {
        return;
    }
    let mut retained = serde_json::Map::new();
    for key in ["dependencies", "optionalDependencies"] {
        if let Some(value) = snapshot.get(key).cloned() {
            retained.insert(key.to_string(), value);
        }
    }
    let candidate = serde_json::Value::Object(retained);
    *snapshot = if serde_json::from_value::<SnapshotEntry>(candidate.clone()).is_ok() {
        candidate
    } else {
        serde_json::json!({})
    };
}

fn discard_invalid_generated_field(
    root: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) {
    let Some(value) = root.get(key).cloned() else { return };
    let mut candidate = serde_json::Map::from_iter([
        ("lockfileVersion".to_owned(), serde_json::json!("9.0")),
        ("importers".to_owned(), serde_json::json!({})),
    ]);
    candidate.insert(key.to_owned(), value);
    if serde_json::from_value::<Lockfile>(serde_json::Value::Object(candidate)).is_err() {
        root.remove(key);
    }
}
