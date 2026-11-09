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
    Lockfile, LockfileResolution, NextPackage, NextSettings, PackageKey, SnapshotEntry, VersionPart,
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

/// [`prepare_value_for_fix`] for a document in the next format, whose
/// generated fields live under `settings` and whose `packages` entries
/// carry their `integrity` apart from a `resolution`.
pub(super) fn prepare_next_value_for_fix(value: &mut serde_json::Value) {
    let Some(root) = value.as_object_mut() else { return };
    reduce_next_settings(root);
    if root
        .get("time")
        .is_some_and(|time| {
            serde_json::from_value::<std::collections::BTreeMap<String, String>>(time.clone())
                .is_err()
        })
    {
        root.remove("time");
    }
    if let Some(packages) = root.get_mut("packages").and_then(serde_json::Value::as_object_mut) {
        packages.retain(|key, entry| reduce_next_package(key, entry));
    }
}

/// Drop each `settings` field that does not decode and keep the rest. A
/// project's `catalog:` dependencies take their versions from the
/// catalogs, so a malformed field beside them must not take them along.
fn reduce_next_settings(root: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(settings) = root.get_mut("settings") else { return };
    let Some(fields) = settings.as_object_mut() else {
        root.remove("settings");
        return;
    };
    fields.retain(|key, field| {
        let candidate = serde_json::Map::from_iter([(key.clone(), field.clone())]);
        serde_json::from_value::<NextSettings>(serde_json::Value::Object(candidate)).is_ok()
    });
}

/// Keep a next-format `packages` entry that decodes and names a
/// resolution, or narrow it to what names and pins the package: its
/// resolution and integrity, with its dependency edges when they decode
/// too. An entry whose resolution neither is written nor follows from its
/// key names nothing and is dropped.
fn reduce_next_package(key: &str, entry: &mut serde_json::Value) -> bool {
    if !names_resolution(key, entry) {
        return false;
    }
    if serde_json::from_value::<NextPackage>(entry.clone()).is_ok() {
        return true;
    }
    const PIN: [&str; 2] = ["resolution", "integrity"];
    const PIN_WITH_EDGES: [&str; 4] =
        ["resolution", "integrity", "dependencies", "optionalDependencies"];
    for keys in [&PIN_WITH_EDGES[..], &PIN[..]] {
        let candidate = retained_fields(entry, keys);
        if serde_json::from_value::<NextPackage>(candidate.clone()).is_ok() {
            *entry = candidate;
            return true;
        }
    }
    false
}

/// Whether the entry under `key` names its resolution: written out, as an
/// integrity alone, or implied by an injected package's `file:` key.
fn names_resolution(key: &str, entry: &serde_json::Value) -> bool {
    entry.get("resolution").is_some()
        || entry.get("integrity").is_some()
        || key
            .parse::<PackageKey>()
            .is_ok_and(|key| matches!(key.suffix.version(), VersionPart::File(_)))
}

fn retained_fields(entry: &serde_json::Value, keys: &[&str]) -> serde_json::Value {
    let retained = keys
        .iter()
        .filter_map(|key| {
            entry
                .get(*key)
                .map(|value| ((*key).to_string(), value.clone()))
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::Value::Object(retained)
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
