//! Pacquet port of pnpm's global-virtual-store directory naming —
//! [`calcGraphNodeHash`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L122-L146)
//! and
//! [`formatGlobalVirtualStorePath`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L155-L160).
//!
//! The engine string contribution is gated by
//! `transitively_requires_build` (private to this crate): when the
//! caller supplies a `built_dep_paths` set (derived from
//! `allowBuilds` / `dangerouslyAllowAllBuilds`), only snapshots
//! that themselves run a build script — or that transitively
//! depend on one — keep the engine in the GVS hash payload.
//! Pure-JS leaves and their pure-JS ancestors hash with
//! `engine = null`, so their GVS directories survive Node.js
//! upgrades and architecture moves. Passing `None` for
//! `built_dep_paths` reproduces the always-include behaviour
//! (matches upstream's
//! [`builtDepPaths === undefined`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L140-L142)
//! branch).

use crate::{
    HashEncoding,
    dep_state::{DepsGraphNode, DepsStateCache, calc_dep_graph_hash, transitively_requires_build},
    hash_object, hash_object_without_sorting,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Compute the hex digest that uniquely identifies one snapshot's
/// position in the global virtual store. Mirrors
/// [`calcGraphNodeHash`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L122-L146).
///
/// The output is the `hash` segment that
/// [`format_global_virtual_store_path`] places after `<name>/<version>/`.
/// Two snapshots that resolve to the same package contents (identical
/// `fullPkgId`s and identical recursive children) hash to the same
/// value and therefore share one directory under
/// `<store>/links/<name>/<version>/` — which is how pnpm and pacquet
/// avoid re-extracting the same tarball once per peer-context.
///
/// `engine` is the candidate `ENGINE_NAME`-shaped string
/// (`<platform>-<arch>-node<major>`). It is included in the hash
/// payload only when the snapshot transitively requires a build —
/// pure-JS subgraphs hash with `engine = null` so their GVS
/// directories survive Node.js upgrades. The gating is driven by:
///
/// - `built_dep_paths`: when `Some`, the set of snapshot keys whose
///   `allowBuilds` entry evaluates to `true` (or every snapshot when
///   `dangerouslyAllowAllBuilds` is on). `None` disables the gating
///   and forces `include_engine = true` — matches upstream's
///   `builtDepPaths === undefined` branch.
/// - `build_required_cache`: install-scoped memoization for the
///   gating walk. Allocated once at the call site and reused across
///   every snapshot in the install — see
///   [`iterateHashedGraphNodes`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L99-L120)
///   for the upstream lifecycle. Untouched when `built_dep_paths`
///   is `None`; callers that don't care can hold a throwaway
///   `let mut cache = HashMap::new();` and pass `&mut cache`.
pub fn calc_graph_node_hash<Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    cache: &mut DepsStateCache<Key>,
    dep_path: &Key,
    engine: Option<&str>,
    built_dep_paths: Option<&HashSet<Key>>,
    build_required_cache: &mut HashMap<Key, bool>,
) -> String
where
    Key: Clone + Eq + std::hash::Hash,
{
    let include_engine = match built_dep_paths {
        None => true,
        Some(set) => transitively_requires_build(
            graph,
            set,
            build_required_cache,
            dep_path,
            &mut HashSet::new(),
        ),
    };
    let engine_value = if include_engine {
        match engine {
            Some(s) => Value::String(s.to_owned()),
            None => Value::Null,
        }
    } else {
        Value::Null
    };
    let deps_hash = calc_dep_graph_hash(graph, cache, &mut HashSet::new(), dep_path);
    let payload = json!({
        "engine": engine_value,
        "deps": deps_hash,
    });
    hash_object_without_sorting(&payload, HashEncoding::Hex)
}

/// Compute the GVS hash for a config dependency that has no children
/// at all. Mirrors
/// [`calcLeafGlobalVirtualStorePath`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L162-L166).
///
/// Unlike [`calc_graph_node_hash`], this doesn't walk a dependency
/// graph — it hashes a single `{ id: full_pkg_id, deps: {} }` node and
/// wraps it in the `{ engine: null, deps }` payload. The env-installer
/// uses it for the platform-specific optional subdeps of a config
/// dependency, which are installed one level deep with no further
/// children of their own.
#[must_use]
pub fn calc_leaf_global_virtual_store_path(full_pkg_id: &str, name: &str, version: &str) -> String {
    let deps_hash = hash_object(&json!({ "id": full_pkg_id, "deps": {} }));
    let payload = json!({ "engine": Value::Null, "deps": deps_hash });
    let hex_digest = hash_object_without_sorting(&payload, HashEncoding::Hex);
    format_global_virtual_store_path(name, version, &hex_digest)
}

/// Compute the GVS hash for a config dependency together with its
/// one-level optional subdeps. Mirrors
/// [`calcGlobalVirtualStorePathWithSubdeps`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L175-L188).
///
/// `subdep_ids` maps each subdep alias to its `full_pkg_id`
/// (`<name>@<version>:<integrity>`). Folding the subdeps into the
/// parent's hash is what keeps a parent pinned to one version from
/// colliding on the same leaf directory when a subdep version changes
/// underneath it — without this, changing a subdep while keeping the
/// parent pinned would silently overwrite the previous sibling
/// symlinks.
#[must_use]
pub fn calc_global_virtual_store_path_with_subdeps(
    full_pkg_id: &str,
    name: &str,
    version: &str,
    subdep_ids: &BTreeMap<String, String>,
) -> String {
    let mut child_hashes = serde_json::Map::new();
    for (alias, child_full_pkg_id) in subdep_ids {
        let child_hash = hash_object(&json!({ "id": child_full_pkg_id, "deps": {} }));
        child_hashes.insert(alias.clone(), Value::String(child_hash));
    }
    let deps_hash = hash_object(&json!({ "id": full_pkg_id, "deps": Value::Object(child_hashes) }));
    let payload = json!({ "engine": Value::Null, "deps": deps_hash });
    let hex_digest = hash_object_without_sorting(&payload, HashEncoding::Hex);
    format_global_virtual_store_path(name, version, &hex_digest)
}

/// Format a global-virtual-store-relative path for a package. Mirrors
/// upstream's
/// [`formatGlobalVirtualStorePath`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L155-L160)
/// — the `@/` prefix on unscoped packages keeps every entry in the
/// shared store at the same `<scope>/<name>/<version>/<hash>` depth,
/// so a single `readdir` pass per level can enumerate the store
/// without special-casing the unscoped path layout.
#[must_use]
pub fn format_global_virtual_store_path(name: &str, version: &str, hex_digest: &str) -> String {
    let prefix = if name.starts_with('@') { "" } else { "@/" };
    format!("{prefix}{name}/{version}/{hex_digest}")
}

#[cfg(test)]
mod tests;
