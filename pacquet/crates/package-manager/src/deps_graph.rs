//! Adapter from pacquet's lockfile structures to
//! [`pacquet_graph_hasher::DepsGraphNode`].
//!
//! `BuildModules`'s `is_built` gate needs to call
//! `calc_dep_state(graph, ...)` per snapshot to compute the
//! side-effects-cache key. Upstream's `DepsGraph` is built from the
//! lockfile via `lockfileToDepGraph` at
//! <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-builder/src/lockfileToDepGraph.ts>;
//! this module ports the subset pacquet needs — `full_pkg_id`
//! derivation per [`createFullPkgId`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-hasher/src/index.ts#L263-L292),
//! and children-link wiring from `SnapshotEntry.dependencies` +
//! `optional_dependencies`.

use pacquet_graph_hasher::{DepsGraphNode, HashEncoding, hash_object_with_encoding};
use pacquet_lockfile::{LockfileResolution, PackageKey, PackageMetadata, SnapshotEntry};
use std::collections::HashMap;

/// Build a `DepsGraph<PackageKey>` from a v9 lockfile's `snapshots`
/// + `packages` sections.
///
/// Each output node carries:
/// - `full_pkg_id` = `<pkg_id>:<integrity>` for registry / tarball
///   resolutions with an integrity, or `<pkg_id>:<hashObject(resolution)>`
///   for git / directory / unintegrity-tarball resolutions. Matches
///   upstream's [`createFullPkgId`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-hasher/src/index.ts#L263-L292).
/// - `children` = alias → child `PackageKey`, walking the snapshot's
///   `dependencies` + `optional_dependencies`. The alias key in the
///   map is the dependency's *alias* (the name under which it gets
///   linked into the parent's `node_modules`), which can differ
///   from the resolved package name for npm-alias deps.
///
/// Snapshots whose metadata entry is missing from `packages` are
/// skipped (the lockfile is malformed; surface that as a build
/// error elsewhere — `BuildModules`'s `is_built` gate will simply
/// miss the cache lookup for those).
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
/// Upstream's [`lockfileToDepGraph`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/deps/graph-builder/src/lockfileToDepGraph.ts#L123-L170)
/// always builds the full graph because its consumers extend
/// beyond cache hashing (hierarchy + hoisting + direct-deps map +
/// bin linking). Pacquet only uses the graph for cache hashing
/// today, so the trimmed walk is sound here — same cache keys,
/// fewer cycles spent.
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

/// Mirrors [`createFullPkgId`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-hasher/src/index.ts#L263-L292).
/// For registry / tarball resolutions the integrity goes verbatim
/// after the package-id; for everything else (git, directory,
/// integrity-less tarball) the resolution is serialized to JSON and
/// run through `hashObject` — pnpm's stable fingerprint for
/// non-integrity resolutions.
///
/// Returns the `pkg_id:<...>` string used as the `id` field in
/// `calc_dep_graph_hash`'s `{ id, deps }` object.
fn full_pkg_id_for(pkg_key: &PackageKey, resolution: &LockfileResolution) -> String {
    // `PackageKey`'s `Display` impl produces `<name>@<ver>` — the
    // same shape upstream's `pkgIdWithPatchHash` carries in pnpm
    // v9 lockfiles. (Pre-v6 lockfiles used the `/<name>/<ver>`
    // shape, but pacquet doesn't parse those.)
    let pkg_id = pkg_key.to_string();
    if let Some(integrity) = resolution.integrity() {
        return format!("{pkg_id}:{integrity}");
    }
    // Fallback for non-integrity resolutions (git, directory). We
    // serialize the resolution to a JSON value and hash it the same
    // way upstream's `hashObject(resolution)` does. Upstream's
    // `hashObject` defaults to base64; pacquet pins the same
    // encoding here for byte-for-byte parity of the resulting
    // `<pkg_id>:<digest>` string.
    let resolution_value = serde_json::to_value(resolution).unwrap_or(serde_json::Value::Null);
    let hash =
        hash_object_with_encoding(&resolution_value, HashEncoding::Base64, /* sort */ true);
    format!("{pkg_id}:{hash}")
}

/// Flatten `SnapshotEntry`'s `dependencies` + `optional_dependencies`
/// into an `alias → PackageKey` map. Mirrors
/// [`lockfileDepsToGraphChildren`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-hasher/src/index.ts#L252-L261)
/// using pacquet's already-typed `SnapshotDepRef` instead of
/// upstream's string reference.
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
        // map. `link:` deps don't have a snapshot key — skip them,
        // mirroring upstream's `if (childDepPath)` guard in
        // `lockfileDepsToGraphChildren`.
        let Some(resolved) = dep_ref.resolve(alias) else {
            continue;
        };
        children.insert(alias.to_string(), resolved);
    }
    children
}

#[cfg(test)]
mod tests;
