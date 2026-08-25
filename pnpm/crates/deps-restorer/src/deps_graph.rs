//! Adapter from pacquet's lockfile structures to
//! [`pnpm_graph_hasher::DepsGraphNode`].
//!
//! `BuildModules`'s `is_built` gate needs to call
//! `calc_dep_state(graph, ...)` per snapshot to compute the
//! side-effects-cache key. This module builds that graph from the
//! lockfile's `snapshots` + `packages` sections — `full_pkg_id`
//! derivation plus children-link wiring from `SnapshotEntry.dependencies`
//! + `optional_dependencies`.

use indexmap::IndexMap;
use pnpm_graph_hasher::{DepsGraphNode, HashEncoding, hash_object_with_encoding};
use pnpm_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgName, SnapshotDepRef, SnapshotEntry,
};
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
///
/// The result is ordered like upstream's
/// `{...dependencies, ...optionalDependencies}` object: each section in
/// lockfile key order, an alias in both sections keeping its position
/// from `dependencies` while taking its value from
/// `optionalDependencies`. Both sections are sorted on disk, so sorting
/// them here restores the order the graph hasher's digests are defined
/// in (see [`pnpm_graph_hasher::DepsGraphNode::children`]).
#[must_use]
pub fn build_children(snapshot: &SnapshotEntry) -> IndexMap<String, PackageKey> {
    build_children_with(snapshot, |alias, dep_ref| dep_ref.resolve(alias))
}

pub(crate) fn build_children_with<Child>(
    snapshot: &SnapshotEntry,
    mut resolve: impl FnMut(&PkgName, &SnapshotDepRef) -> Option<Child>,
) -> IndexMap<String, Child> {
    let mut children = IndexMap::new();
    extend_children(&mut children, snapshot.dependencies.as_ref(), &mut resolve);
    extend_children(&mut children, snapshot.optional_dependencies.as_ref(), &mut resolve);
    children
}

fn extend_children<Child>(
    children: &mut IndexMap<String, Child>,
    deps: Option<&HashMap<PkgName, SnapshotDepRef>>,
    resolve: &mut impl FnMut(&PkgName, &SnapshotDepRef) -> Option<Child>,
) {
    let Some(deps) = deps else { return };
    let mut section: Vec<(String, Child)> = deps
        .iter()
        .filter_map(|(alias, dep_ref)| Some((alias.to_string(), resolve(alias, dep_ref)?)))
        .collect();
    section.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
    children.extend(section);
}

/// A snapshot map's entries in lockfile key order.
///
/// Pacquet parses the lockfile into `HashMap`s, so iterating one
/// directly hands the graph hasher a different entry-point order on
/// every run — and the digests it computes for cyclic subgraphs depend
/// on that order (see
/// [`pnpm_graph_hasher::DepsGraphNode::children`]). Sorting by the
/// rendered snapshot key reproduces how pnpm writes — and therefore
/// iterates — the `snapshots:` section.
#[must_use]
pub fn in_lockfile_order<Value>(
    snapshots: &HashMap<PackageKey, Value>,
) -> Vec<(&PackageKey, &Value)> {
    let mut entries: Vec<(String, &PackageKey, &Value)> =
        snapshots.iter().map(|(key, value)| (key.to_string(), key, value)).collect();
    entries.sort_unstable_by(|(left, ..), (right, ..)| left.cmp(right));
    entries.into_iter().map(|(_, key, value)| (key, value)).collect()
}

#[cfg(test)]
mod tests;
