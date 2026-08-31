use crate::object_hasher::hash_object;
use indexmap::IndexMap;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub const DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX: &str = "dependency-side-effects:v1:";

/// Per-node identifier carrying everything [`calc_dep_state`] needs to
/// hash a snapshot.
///
/// `full_pkg_id` is the fingerprint used as the `id` field in the
/// recursive hash — `<pkgIdWithPatchHash>:<integrity>` for packages with
/// an integrity (`registry` resolution), or
/// `<pkgIdWithPatchHash>:<hashObject(resolution)>` for resolutions
/// without one (e.g. git refs). A variations resolution uses the integrity of
/// the selected platform variant. Pacquet's caller composes this before
/// passing it in; the hasher itself is opaque to how it was computed.
///
/// `children` maps alias → dep-graph key for the snapshot's
/// children. Pacquet's natural input shape is the lockfile's
/// `snapshots[].dependencies` + `optionalDependencies` flattened,
/// with each value resolved to the snapshot key it points at.
///
/// The map is insertion-ordered because iteration order is part of the
/// hash contract: a node reached while one of its own ancestors is
/// mid-walk hashes with its children truncated, so inside a dependency
/// cycle the digest a node settles on depends on the order its parents
/// were visited in. Upstream's node holds a plain JS object built by
/// spreading `dependencies` then `optionalDependencies`, so callers must
/// insert in that same order (each section in lockfile key order) for
/// the digests to match.
///
/// Owns its strings so a caller building the graph from a lockfile
/// doesn't have to keep a separate `String` arena alive for the
/// duration of the hash walk.
pub struct DepsGraphNode<Key> {
    pub full_pkg_id: String,
    pub children: IndexMap<String, Key>,
}

/// Memoized per-depPath state cache: the result of `hash_object` for
/// each visited node is stashed so the recursive walk over diamond-shaped
/// graphs stays linear.
pub type DepsStateCache<Key> = HashMap<Key, String>;

/// Inputs to [`calc_dep_state`].
pub struct CalcDepStateOptions<'a> {
    /// Output of [`crate::engine_name()`] — the platform / arch /
    /// node version prefix. Always part of the result.
    pub engine_name: &'a str,
    /// SHA-256 hex of the patch file for this package (when present).
    /// Appended as `;patch=<hash>`.
    pub patch_file_hash: Option<&'a str>,
    /// Whether to include the recursive dep-graph hash as
    /// `;deps=<hash>`. Set to `hasSideEffects`
    /// (i.e. `!ignoreScripts && requiresBuild`).
    pub include_dep_graph_hash: bool,
}

/// Compute the side-effects cache key for a snapshot.
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

/// Compute the machine-independent lookup key for a remotely shareable
/// dependency build.
///
/// Unlike [`calc_dep_state`], this key deliberately excludes the engine name.
/// The signed artifact advertises platform compatibility separately, allowing
/// one compatible artifact to serve more than one exact host identity.
///
/// `graph` must contain `dep_path`; a missing root is a caller error and
/// panics rather than producing the same key for every missing dependency.
/// The walk uses a private per-call cache, so the result is independent of
/// earlier roots and platform-selected graphs. `graph` is not mutated.
///
/// The returned key starts with [`DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX`],
/// followed by the recursive dependency-graph hash and, when supplied, the
/// patch-file hash. The selected variation's source integrity is included in
/// the graph hash through [`DepsGraphNode::full_pkg_id`].
pub fn calc_dep_state_input_key<Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    dep_path: &Key,
    patch_file_hash: Option<&str>,
) -> String
where
    Key: Clone + Eq + std::hash::Hash,
{
    assert!(
        graph.contains_key(dep_path),
        "dependency side-effects input-key root is not present in the graph",
    );
    let deps_hash = calc_dep_graph_hash(graph, &mut HashMap::new(), &mut HashSet::new(), dep_path);
    let mut result = format!("{DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX}deps={deps_hash}");
    if let Some(patch) = patch_file_hash {
        result.push_str(";patch=");
        result.push_str(patch);
    }
    result
}

/// Recursive helper for the `deps=` portion.
///
/// Hashes each node as `hash_object({ id, deps })` where `deps` is
/// the alias→child-hash map. `parents` breaks dependency cycles —
/// when a node would re-enter via its own ancestor, the child's
/// contribution becomes `""` (the "node not in graph" guard returns
/// the empty string).
///
/// **Visit order is part of the digest.** A node reached while one of
/// its own ancestors is mid-walk hashes with its children truncated,
/// and that truncated digest is what lands in `cache` until the
/// outermost visit overwrites it — so inside a dependency cycle the
/// digest a node ends up with depends on which node the walk entered
/// the cycle from. Upstream is deterministic because JS objects
/// iterate in insertion order; pacquet reproduces that by keeping
/// [`DepsGraphNode::children`] insertion-ordered and by having callers
/// drive the per-snapshot walks in lockfile key order (see
/// [`crate::warm_deps_state_cache`]). Feeding this walk a
/// `HashMap`-ordered graph instead would give the same lockfile a
/// different global-virtual-store slot on every run, and each install
/// would re-import whatever landed on a fresh slot path.
///
/// Exposed at `pub(crate)` so the global-virtual-store path hasher
/// (`crate::global_virtual_store_path`) can share the same recursion
/// and cache — both [`calc_dep_state`] and `calc_graph_node_hash` are
/// its only callers within this crate.
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

/// Populate `cache` by walking `keys` in order, so that later lookups
/// see the same digests no matter what order — or from how many
/// threads — the callers ask for them.
///
/// [`calc_dep_state`] and [`crate::calc_graph_node_hash`] both memoize
/// into a shared install-scoped cache, and for cyclic subgraphs the
/// digest they store depends on the entry point that reached the node
/// first (see [`DepsGraphNode::children`] for why). Callers that would
/// otherwise enter the graph in an unordered — or concurrent — sequence
/// prime the cache through here first, passing the snapshot keys in
/// lockfile order.
pub fn warm_deps_state_cache<'a, Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    cache: &mut DepsStateCache<Key>,
    keys: impl IntoIterator<Item = &'a Key>,
) where
    Key: 'a + Clone + Eq + std::hash::Hash,
{
    // One `parents` set for the whole warm-up: each walk leaves it empty
    // again, and a key that is already memoized needs no walk at all.
    let mut parents = HashSet::new();
    for key in keys {
        if cache.contains_key(key) {
            continue;
        }
        calc_dep_graph_hash(graph, cache, &mut parents, key);
    }
}

/// Return every node that is, or transitively depends on, a node
/// in `built_dep_paths`.
///
/// The result controls whether [`crate::calc_graph_node_hash`] includes
/// the engine in a global-virtual-store hash. It is computed as one
/// graph-wide fixed point so dependency cycles cannot produce different
/// answers for different entry points.
///
/// A key with no node in `graph` is kept and still marks whatever depends
/// on it: the built set comes from the allow-build policy rather than the
/// graph, so the two can disagree.
#[must_use]
pub fn build_required_dep_paths<Key>(
    graph: &HashMap<Key, DepsGraphNode<Key>>,
    built_dep_paths: &HashSet<Key>,
) -> HashSet<Key>
where
    Key: Clone + Eq + std::hash::Hash,
{
    if built_dep_paths.is_empty() {
        return HashSet::new();
    }

    let mut parents_by_child: HashMap<&Key, Vec<&Key>> = HashMap::new();
    for (parent, node) in graph {
        for child in node.children.values() {
            parents_by_child.entry(child).or_default().push(parent);
        }
    }

    let mut build_required: HashSet<&Key> = built_dep_paths.iter().collect();
    let mut pending: Vec<&Key> = built_dep_paths.iter().collect();
    while let Some(child) = pending.pop() {
        let Some(parents) = parents_by_child.get(child) else {
            continue;
        };
        for parent in parents {
            if build_required.insert(parent) {
                pending.push(parent);
            }
        }
    }
    build_required.into_iter().cloned().collect()
}

#[cfg(test)]
mod tests;
