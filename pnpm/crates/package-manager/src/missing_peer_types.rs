//! The `addMissingPeerTypes` manifest transform.
//!
//! A package that peer-depends on `react` reads React's types from
//! `@types/react`, which it rarely declares. Without a declaration the
//! types package is not linked next to the package, and TypeScript can only
//! find it by walking up to the project's `node_modules`. The global virtual
//! store puts packages outside the project, where that walk never reaches it.
//!
//! Yarn applies the same rule to every package in `normalizePackage`.

use serde_json::{Map, Value};
use std::sync::Arc;

const TYPES_SCOPE: &str = "@types/";

/// Add `@types/X` as an optional peer with the range `*` for every peer `X`,
/// returning the original `Arc` when there is nothing to add.
///
/// `X` is skipped when it is itself an `@types` package, or when the manifest
/// already names `@types/X` in `dependencies`, `optionalDependencies`,
/// `peerDependencies`, or `peerDependenciesMeta`.
#[must_use]
pub(crate) fn add_missing_peer_types(manifest: Arc<Value>) -> Arc<Value> {
    let missing = missing_peer_types(&manifest);
    if missing.is_empty() {
        return manifest;
    }
    let mut manifest = manifest;
    if let Some(map) = Arc::make_mut(&mut manifest).as_object_mut() {
        insert_optional_peers(map, missing);
    }
    manifest
}

fn missing_peer_types(manifest: &Value) -> Vec<String> {
    let Some(peers) = manifest.get("peerDependencies").and_then(Value::as_object) else {
        return Vec::new();
    };
    peers
        .keys()
        .filter(|name| !name.starts_with(TYPES_SCOPE))
        .map(|name| types_package_name(name))
        .filter(|types_name| !declares(manifest, types_name))
        .collect()
}

/// `@types/react` for `react`, `@types/babel__core` for `@babel/core`.
fn types_package_name(name: &str) -> String {
    match name.strip_prefix('@') {
        Some(scoped) => format!("{TYPES_SCOPE}{}", scoped.replacen('/', "__", 1)),
        None => format!("{TYPES_SCOPE}{name}"),
    }
}

fn declares(manifest: &Value, name: &str) -> bool {
    ["dependencies", "optionalDependencies", "peerDependencies", "peerDependenciesMeta"]
        .into_iter()
        .any(|field| {
            manifest
                .get(field)
                .and_then(Value::as_object)
                .is_some_and(|entries| entries.contains_key(name))
        })
}

fn insert_optional_peers(manifest: &mut Map<String, Value>, names: Vec<String>) {
    for name in names {
        object_field(manifest, "peerDependencies")
            .insert(name.clone(), Value::String("*".to_string()));
        object_field(manifest, "peerDependenciesMeta")
            .insert(name, serde_json::json!({ "optional": true }));
    }
}

fn object_field<'manifest>(
    manifest: &'manifest mut Map<String, Value>,
    field: &str,
) -> &'manifest mut Map<String, Value> {
    let entry = manifest
        .entry(field)
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry.as_object_mut().expect("the field was just made an object")
}

#[cfg(test)]
mod tests;
