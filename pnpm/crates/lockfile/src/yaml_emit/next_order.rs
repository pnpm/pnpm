//! The key order of the next lockfile format's `packages` entries.

use super::{Map, Value, map_values, sort_by_priority, sort_deep_keys, sort_direct_keys};

/// The key order of a `packages` entry in the next lockfile format, which
/// has no `snapshots`: metadata, then the resolved graph. Nested maps are
/// alphabetical, except the items of `snapshots`, which follow this order too.
const NEXT_PACKAGE_KEYS: [&str; 22] = [
    "resolution",
    "integrity",
    "version",
    "engines",
    "cpu",
    "os",
    "libc",
    "deprecated",
    "hasBin",
    "prepare",
    "bundledDependencies",
    "peerDependencies",
    "peerDependenciesMeta",
    "id",
    "patched",
    "optional",
    "dependencies",
    "optionalDependencies",
    "peers",
    "transitivePeers",
    "missingPeers",
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
                (_, value) => sort_deep_keys(value),
            };
            (key, value)
        })
        .collect();
    Value::Object(sorted)
}

/// Sorts the `packages` entries of a next-format document in place, and says
/// whether the document is one.
pub(super) fn sort_next_packages(root: &mut Map<String, Value>) -> bool {
    if !is_next_format(root) {
        return false;
    }
    if let Some(Value::Object(packages)) = root.remove("packages") {
        let sorted = map_values(sort_direct_keys(packages), sort_next_package);
        root.insert("packages".to_string(), Value::Object(sorted));
    }
    true
}
