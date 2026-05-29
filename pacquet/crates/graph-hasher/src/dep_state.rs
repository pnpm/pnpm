use crate::object_hasher::hash_object;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

/// Per-node identifier carrying everything `calc_dep_state` needs to
/// hash a snapshot. Mirrors the relevant subset of pnpm's
/// `DepsGraphNode` at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-hasher/src/index.ts#L12-L19>.
///
/// `full_pkg_id` is the upstream-shaped fingerprint used as the
/// `id` field in the recursive hash — `<pkgIdWithPatchHash>:<integrity>`
/// for packages with an integrity (`registry` resolution),
/// or `<pkgIdWithPatchHash>:<hashObject(resolution)>` for resolutions
/// without one (e.g. git refs). Pacquet's caller composes this
/// before passing it in; the hasher itself is opaque to how it was
/// computed.
///
/// `children` maps alias → dep-graph key for the snapshot's
/// children. Pacquet's natural input shape is the lockfile's
/// `snapshots[].dependencies` + `optionalDependencies` flattened,
/// with each value resolved to the snapshot key it points at.
///
/// Owns its strings so a caller building the graph from a lockfile
/// doesn't have to keep a separate `String` arena alive for the
/// duration of the hash walk.
pub struct DepsGraphNode<Key> {
    pub full_pkg_id: String,
    pub children: HashMap<String, Key>,
}

/// Memoized per-depPath state cache. Mirrors pnpm's
/// [`DepsStateCache`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-hasher/src/index.ts#L21-L23):
/// the result of `hash_object` for each visited node is stashed so
/// the recursive walk over diamond-shaped graphs stays linear.
pub type DepsStateCache<Key> = HashMap<Key, String>;

/// Inputs to [`calc_dep_state`]. Mirrors the option bag at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-hasher/src/index.ts#L29-L33>.
pub struct CalcDepStateOptions<'a> {
    /// Output of [`crate::engine_name()`] — the platform / arch /
    /// node version prefix. Always part of the result.
    pub engine_name: &'a str,
    /// SHA-256 hex of the patch file for this package (when present).
    /// Appended as `;patch=<hash>`.
    pub patch_file_hash: Option<&'a str>,
    /// Whether to include the recursive dep-graph hash as
    /// `;deps=<hash>`. Upstream sets this to `hasSideEffects`
    /// (i.e. `!ignoreScripts && requiresBuild`) at
    /// `building/during-install/src/index.ts:202`.
    pub include_dep_graph_hash: bool,
}

/// Mirrors `calcDepState` at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-hasher/src/index.ts#L25-L44>.
///
/// Returns the cache key for the side-effects cache. Format:
/// `<engine_name>[;deps=<hash>][;patch=<hash>]`. Byte-for-byte
/// parity with pnpm is required — the key is persisted on disk and
/// shared with pnpm.
pub fn calc_dep_state<Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    cache: &mut DepsStateCache<Key>,
    dep_path: &Key,
    opts: &CalcDepStateOptions<'_>,
) -> String
where
    Key: Clone + Eq + std::hash::Hash,
{
    let mut result = opts.engine_name.to_string();
    if opts.include_dep_graph_hash {
        let deps_hash = calc_dep_graph_hash(graph, cache, &mut HashSet::new(), dep_path);
        result.push_str(";deps=");
        result.push_str(&deps_hash);
    }
    if let Some(patch) = opts.patch_file_hash {
        result.push_str(";patch=");
        result.push_str(patch);
    }
    result
}

/// Recursive helper for the `deps=` portion. Mirrors
/// `calcDepGraphHash` at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-hasher/src/index.ts#L46-L80>.
///
/// Hashes each node as `hashObject({ id, deps })` where `deps` is
/// the alias→child-hash map. `parents` breaks dependency cycles —
/// when a node would re-enter via its own ancestor, the child's
/// contribution becomes `""` (matching upstream's "node not in
/// graph" guard at line 55, which returns the empty string).
///
/// Exposed at `pub(crate)` so the global-virtual-store path hasher
/// (`crate::global_virtual_store_path`) can share the same recursion
/// and cache. Keeping it `pub(crate)` rather than `pub` mirrors the
/// upstream module layout, where `calcDepGraphHash` is private to
/// `deps/graph-hasher/src/index.ts` and both `calcDepState` and
/// `calcGraphNodeHash` are file-internal callers.
pub(crate) fn calc_dep_graph_hash<Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    cache: &mut DepsStateCache<Key>,
    parents: &mut HashSet<String>,
    dep_path: &Key,
) -> String
where
    Key: Clone + Eq + std::hash::Hash,
{
    if let Some(cached) = cache.get(dep_path) {
        return cached.clone();
    }
    let Some(node) = graph.get(dep_path) else {
        return String::new();
    };
    let mut deps_obj = serde_json::Map::new();
    if !node.children.is_empty() && !parents.contains(&node.full_pkg_id) {
        // Push our `full_pkg_id` for the duration of this subtree
        // so cycles short-circuit on the second visit.
        let inserted = parents.insert(node.full_pkg_id.clone());
        for (alias, child_key) in &node.children {
            let child_hash = calc_dep_graph_hash(graph, cache, parents, child_key);
            deps_obj.insert(alias.clone(), Value::String(child_hash));
        }
        if inserted {
            parents.remove(&node.full_pkg_id);
        }
    }
    let hashed = hash_object(&json!({
        "id": node.full_pkg_id.clone(),
        "deps": Value::Object(deps_obj),
    }));
    cache.insert(dep_path.clone(), hashed);
    cache.get(dep_path).expect("just inserted").clone()
}

/// Recursive helper used by [`crate::calc_graph_node_hash`] to decide
/// whether a snapshot's engine string should contribute to its global-
/// virtual-store hash. Mirrors upstream's
/// [`transitivelyRequiresBuild`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L221-L249).
///
/// Returns `true` if `dep_path` is either in `built_dep_paths`
/// directly, or transitively depends on a snapshot that is. The
/// returned boolean drives `includeEngine` at upstream's
/// [`calcGraphNodeHash`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-hasher/src/index.ts#L140-L142)
/// — pure-JS leaves (and their pure-JS ancestors) get
/// `engine = null`, so their GVS hashes survive Node.js upgrades and
/// architecture moves. Snapshots that *might* run a postinstall
/// script keep `engine = ENGINE_NAME` so the hash partitions them by
/// host environment.
///
/// The cycle guard uses `dep_path` itself, not `node.full_pkg_id`
/// (unlike [`calc_dep_graph_hash`]). Upstream picked `depPath`
/// because the same pkg id reachable through two different peer
/// contexts is two distinct nodes — once one is mid-walk we still
/// want to recurse into the other.
///
/// On cycle hit (`parents.contains(dep_path)`) the function returns
/// `false` *without* caching. The "false in this particular cycle
/// rotation" answer isn't the canonical one — a sibling visit might
/// still find a builder upstream, and caching `false` here would
/// poison the next visit at the same key.
///
/// `cache` is install-scoped and threaded across every snapshot
/// visited inside one [`crate::calc_graph_node_hash`] walk. `parents`
/// is the per-walk cycle-tracking set — callers always pass a fresh
/// empty `HashSet`, the function inserts/removes `dep_path` around
/// the recursion.
pub(crate) fn transitively_requires_build<Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    built_dep_paths: &HashSet<Key>,
    cache: &mut HashMap<Key, bool>,
    dep_path: &Key,
    parents: &mut HashSet<Key>,
) -> bool
where
    Key: Clone + Eq + std::hash::Hash,
{
    if let Some(&cached) = cache.get(dep_path) {
        return cached;
    }
    if built_dep_paths.contains(dep_path) {
        cache.insert(dep_path.clone(), true);
        return true;
    }
    let Some(node) = graph.get(dep_path) else {
        cache.insert(dep_path.clone(), false);
        return false;
    };
    if parents.contains(dep_path) {
        return false;
    }
    parents.insert(dep_path.clone());
    let mut result = false;
    for child in node.children.values() {
        if transitively_requires_build(graph, built_dep_paths, cache, child, parents) {
            result = true;
            break;
        }
    }
    parents.remove(dep_path);
    cache.insert(dep_path.clone(), result);
    result
}

#[cfg(test)]
mod tests;
