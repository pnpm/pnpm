//! The key order of the next lockfile format's `packages` entries.

use super::{Map, Value, map_values, sort_by_priority, sort_deep_keys, sort_direct_keys};

/// The key order of a `packages` entry in the next lockfile format, which
/// has no `snapshots`: metadata, then the resolved graph. Nested maps are
/// alphabetical, except the items of `snapshots`, which follow this order too,
/// and the entries of `peerDependencies`, which keep `specifier` before
/// `optional`.
const NEXT_PACKAGE_KEYS: [&str; 16] = [
    "resolution",
    "integrity",
    "version",
    "engines",
    "cpu",
    "os",
    "libc",
    "hasBin",
    "prepare",
    "bundledDependencies",
    "peerDependencies",
    "id",
    "optional",
    "dependencies",
    "optionalDependencies",
    "snapshots",
];

/// A next-format document: it declares the next version and has no
/// `snapshots`. The current lockfile under `node_modules/.pnpm` records the
/// next version with today's shape, and keeps today's key order.
fn is_next_format(root: &Map<String, Value>) -> bool {
    !root.contains_key("snapshots")
        && root
            .get("lockfileVersion")
            .and_then(Value::as_str)
            .is_some_and(|version| version.starts_with("10."))
}

fn sort_next_package(entry: Value) -> Value {
    let Value::Object(map) = entry else { return entry };
    let sorted = sort_by_priority(map, &NEXT_PACKAGE_KEYS, false)
        .into_iter()
        .map(|(key, value)| {
            let value = match (key.as_str(), value) {
                ("snapshots", Value::Array(items)) => Value::Array(
                    items
                        .into_iter()
                        .map(sort_next_package)
                        .collect(),
                ),
                (
                    "peerDependencies" | "dependencies" | "optionalDependencies",
                    Value::Object(entries),
                ) => Value::Object(sort_direct_keys(entries)),
                (_, value) => sort_deep_keys(value),
            };
            (key, value)
        })
        .collect();
    Value::Object(sorted)
}

/// Sorts one entry of a section.
type SortEntry = fn(Value) -> Value;

/// An importer's `name` first, then its keys and each dependency group's
/// names, alphabetical. A dependency written as a map keeps `specifier`,
/// `version`, `with`.
fn sort_next_importer(entry: Value) -> Value {
    let Value::Object(map) = entry else { return entry };
    let sorted = sort_by_priority(map, &["name"], false)
        .into_iter()
        .map(|(key, value)| {
            let value = match (key.as_str(), value) {
                (
                    "dependencies" | "devDependencies" | "optionalDependencies",
                    Value::Object(dependencies),
                ) => Value::Object(sort_direct_keys(dependencies)),
                (_, value) => sort_deep_keys(value),
            };
            (key, value)
        })
        .collect();
    Value::Object(sorted)
}

/// The maps `settings` holds in the next format, written after today's
/// settings, in this order.
const NEXT_SETTINGS_RECORDS: [&str; 6] = [
    "catalogs",
    "overrides",
    "patchedDependencies",
    "packageExtensionsChecksum",
    "pnpmfileChecksum",
    "ignoredOptionalDependencies",
];

/// Today's settings alphabetical, then the recorded sections in
/// [`NEXT_SETTINGS_RECORDS`] order: `catalogs` by catalog and entry,
/// `patchedDependencies` by key, `overrides` as configured.
fn sort_next_settings(settings: Map<String, Value>) -> Map<String, Value> {
    let mut sorted = sort_direct_keys(settings);
    let records: Vec<(String, Value)> = NEXT_SETTINGS_RECORDS
        .iter()
        .filter_map(|key| {
            sorted
                .shift_remove(*key)
                .map(|value| ((*key).to_string(), value))
        })
        .collect();
    for (key, value) in records {
        let value = match (key.as_str(), value) {
            ("catalogs", Value::Object(catalogs)) => {
                Value::Object(map_values(sort_direct_keys(catalogs), sort_deep_keys))
            }
            ("patchedDependencies", Value::Object(patches)) => {
                Value::Object(sort_direct_keys(patches))
            }
            (_, value) => value,
        };
        sorted.insert(key, value);
    }
    sorted
}

/// Sorts the `packages`, `importers` and `settings` of a next-format
/// document in place, and says whether the document is one.
pub(super) fn sort_next_sections(root: &mut Map<String, Value>) -> bool {
    if !is_next_format(root) {
        return false;
    }
    if let Some(Value::Object(settings)) = root.remove("settings") {
        root.insert("settings".to_string(), Value::Object(sort_next_settings(settings)));
    }
    if let Some(Value::Object(time)) = root.remove("time") {
        root.insert("time".to_string(), Value::Object(sort_direct_keys(time)));
    }
    let sorts: [(&str, SortEntry); 2] =
        [("packages", sort_next_package), ("importers", sort_next_importer)];
    for (section, sort) in sorts {
        if let Some(Value::Object(entries)) = root.remove(section) {
            let sorted = map_values(sort_direct_keys(entries), sort);
            root.insert(section.to_string(), Value::Object(sorted));
        }
    }
    true
}
