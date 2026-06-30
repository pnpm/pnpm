//! Adapter from pacquet's lockfile structures to
//! [`pacquet_graph_hasher::DepsGraphNode`].
//!
//! `BuildModules`'s `is_built` gate needs to call
//! `calc_dep_state(graph, ...)` per snapshot to compute the
//! side-effects-cache key. This module builds that graph from the
//! lockfile's `snapshots` + `packages` sections — `full_pkg_id`
//! derivation plus children-link wiring from `SnapshotEntry.dependencies`
//! + `optional_dependencies`.

use pacquet_graph_hasher::{DepsGraphNode, HashEncoding, hash_object_with_encoding};
use pacquet_lockfile::{LockfileResolution, PackageKey, PackageMetadata, SnapshotEntry};
use std::collections::HashMap;

/// Build a `DepsGraph<PackageKey>` from a v9 lockfile's `snapshots`
/// + `packages` sections.
///
/// The alias key in a node's `children` map is the dependency's
/// *alias* (the name under which it gets linked into the parent's
/// `node_modules`), which can differ from the resolved package name
/// for npm-alias deps.
///
/// Snapshots whose metadata entry is missing from `packages` are
/// skipped silently. This is safe: the lockfile is malformed, and
/// `BuildModules`'s `is_built` gate then misses the cache lookup for
/// that snapshot and falls through to "rebuild".
#[must_use]
pub fn build_deps_graph(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> HashMap<PackageKey, DepsGraphNode<PackageKey>> {
    let mut graph = HashMap::with_capacity(snapshots.len());
    for (snapshot_key, snapshot) in snapshots {
        if let Some(node) = build_node(snapshot_key, snapshot, packages) {
            graph.insert(snapshot_key.clone(), node);
        }
    }
    graph
}

/// Build the `DepsGraph` for only the forward closure of `roots`
/// — the union of every snapshot transitively reachable through
/// `dependencies` + `optional_dependencies` starting from any root.
///
/// `BuildModules` uses this for the side-effects cache READ /
/// WRITE gates so the O(|snapshots|) walk doesn't run on the
/// pure-JS install case where no snapshot is `requires_build`.
/// `calc_dep_state` only ever recurses into a node's own
/// closure, so the bounded graph produces the exact same cache
/// keys as the full graph for every root — observable behavior
/// matches [`build_deps_graph`] for the inputs we care about.
///
/// Pacquet only uses the graph for cache hashing today, so the
/// trimmed walk is sound here — same cache keys, fewer cycles spent.
pub fn build_deps_subgraph<Iter>(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    roots: Iter,
) -> HashMap<PackageKey, DepsGraphNode<PackageKey>>
where
    Iter: IntoIterator<Item = PackageKey>,
{
    let mut graph: HashMap<PackageKey, DepsGraphNode<PackageKey>> = HashMap::new();
    let mut queue: std::collections::VecDeque<PackageKey> = roots.into_iter().collect();
    while let Some(key) = queue.pop_front() {
        if graph.contains_key(&key) {
            continue;
        }
        let Some(snapshot) = snapshots.get(&key) else { continue };
        let Some(node) = build_node(&key, snapshot, packages) else { continue };
        // Enqueue every child the new node points at. Repeat-enqueues
        // are cheap — the `graph.contains_key` guard at the top of
        // the loop discards them.
        for child_key in node.children.values() {
            if !graph.contains_key(child_key) {
                queue.push_back(child_key.clone());
            }
        }
        graph.insert(key, node);
    }
    graph
}

fn build_node(
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> Option<DepsGraphNode<PackageKey>> {
    let metadata_key = snapshot_key.without_peer();
    let metadata = packages.get(&metadata_key)?;
    let full_pkg_id = full_pkg_id_for(&metadata_key, &metadata.resolution);
    let children = build_children(snapshot);
    Some(DepsGraphNode { full_pkg_id, children })
}

/// Returns the `pkg_id:<...>` string used as the `id` field in
/// `calc_dep_graph_hash`'s `{ id, deps }` object.
fn full_pkg_id_for(pkg_key: &PackageKey, resolution: &LockfileResolution) -> String {
    // `PackageKey`'s `Display` impl produces `<name>@<ver>` — the
    // shape the `pkgIdWithPatchHash` carries in v9 lockfiles. (Pre-v6
    // lockfiles used the `/<name>/<ver>` shape, but pacquet doesn't
    // parse those.)
    let pkg_id = pkg_key.to_string();
    if let Some(integrity) = resolution.integrity() {
        return format!("{pkg_id}:{integrity}");
    }
    // Fallback for non-integrity resolutions (git, directory). We
    // serialize the resolution to a JSON value and hash it. The hash
    // is base64-encoded, the encoding the resulting
    // `<pkg_id>:<digest>` string requires.
    let resolution_value = serde_json::to_value(resolution).unwrap_or(serde_json::Value::Null);
    let hash =
        hash_object_with_encoding(&resolution_value, HashEncoding::Base64, /* sort */ true);
    format!("{pkg_id}:{hash}")
}

/// Flatten `SnapshotEntry`'s `dependencies` + `optional_dependencies`
/// into an `alias → PackageKey` map, using pacquet's already-typed
/// `SnapshotDepRef`.
fn build_children(snapshot: &SnapshotEntry) -> HashMap<String, PackageKey> {
    let mut children = HashMap::new();
    let dep_entries = snapshot
        .dependencies
        .iter()
        .flat_map(|m| m.iter())
        .chain(snapshot.optional_dependencies.iter().flat_map(|m| m.iter()));
    for (alias, dep_ref) in dep_entries {
        // `SnapshotDepRef::resolve` returns the `PkgNameVerPeer`
        // (= `PackageKey`) the alias points at in the `snapshots:`
        // map. `link:` deps don't have a snapshot key — skip them.
        let Some(resolved) = dep_ref.resolve(alias) else {
            continue;
        };
        children.insert(alias.to_string(), resolved);
    }
    children
}

#[cfg(test)]
mod tests;
