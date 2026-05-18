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
    cache.insert(dep_path.clone(), hashed.clone());
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
mod tests {
    use super::{CalcDepStateOptions, DepsGraphNode, calc_dep_state, transitively_requires_build};
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    /// Engine-only key (no dep graph, no patch). Pure prefix path
    /// for the cheapest cache lookup. Mirrors the "include_dep_graph_hash:
    /// false" path at
    /// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/graph-hasher/src/index.ts#L36>.
    #[test]
    fn engine_only_key() {
        let graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        let mut cache = HashMap::new();
        let result = calc_dep_state(
            &graph,
            &mut cache,
            &"foo@1.0.0".to_string(),
            &CalcDepStateOptions {
                engine_name: "darwin;arm64;node20",
                patch_file_hash: None,
                include_dep_graph_hash: false,
            },
        );
        assert_eq!(result, "darwin;arm64;node20");
    }

    /// Patch hash gets appended as `;patch=<hash>`. Combined with
    /// the engine prefix when there's no dep graph hash. Mirrors
    /// lines 40-42 of `calcDepState`.
    #[test]
    fn patch_appended_without_dep_graph_hash() {
        let graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        let mut cache = HashMap::new();
        let result = calc_dep_state(
            &graph,
            &mut cache,
            &"foo@1.0.0".to_string(),
            &CalcDepStateOptions {
                engine_name: "linux;x64;node22",
                patch_file_hash: Some("sha256-abc"),
                include_dep_graph_hash: false,
            },
        );
        assert_eq!(result, "linux;x64;node22;patch=sha256-abc");
    }

    /// Dep-graph hash for a leaf (no children) is `hash_object({
    /// id, deps: {} })`. Both sites that consult `deps={}` (the
    /// leaf case at `calcLeafGlobalVirtualStorePath` and the
    /// children-elided case for cycle/missing-node) must agree.
    #[test]
    fn dep_graph_hash_for_leaf_uses_id_and_empty_deps() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        graph.insert(
            "leaf@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "leaf@1.0.0:sha512-leaf".to_string(),
                children: HashMap::new(),
            },
        );
        let mut cache = HashMap::new();
        let result = calc_dep_state(
            &graph,
            &mut cache,
            &"leaf@1.0.0".to_string(),
            &CalcDepStateOptions {
                engine_name: "darwin;arm64;node20",
                patch_file_hash: None,
                include_dep_graph_hash: true,
            },
        );
        // Prefix preserved, deps= section appended.
        let parts: Vec<&str> = result.split(';').collect();
        assert!(parts.len() == 4, "expected `<plat>;<arch>;node<n>;deps=<hash>`, got {result:?}");
        assert!(parts[3].starts_with("deps="), "fourth segment must be `deps=...`: {result:?}");
        assert!(parts[3][5..].len() >= 40, "hash payload must be non-trivial: {result:?}");
    }

    /// Memoization at the cache layer: `calc_dep_graph_hash` writes
    /// each node's hash on first visit and returns the cached
    /// value on re-visit. Two leaf nodes with the same
    /// `full_pkg_id` must agree.
    #[test]
    fn cache_makes_repeat_calls_byte_equal() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        graph.insert(
            "leaf@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "leaf@1.0.0:sha512-x".to_string(),
                children: HashMap::new(),
            },
        );
        let mut cache = HashMap::new();
        let opts = CalcDepStateOptions {
            engine_name: "darwin;arm64;node20",
            patch_file_hash: None,
            include_dep_graph_hash: true,
        };
        let a = calc_dep_state(&graph, &mut cache, &"leaf@1.0.0".to_string(), &opts);
        let b = calc_dep_state(&graph, &mut cache, &"leaf@1.0.0".to_string(), &opts);
        assert_eq!(a, b);
        assert_eq!(cache.len(), 1, "cache must hold exactly the one leaf entry");
    }

    /// Diamond graph: root depends on a and b, both depend on c.
    /// Both alias→child entries on the root must agree on the c
    /// node's hash, and the recursion must terminate.
    #[test]
    fn diamond_graph_resolves_consistently() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        let mut root_children = HashMap::new();
        root_children.insert("a".to_string(), "a@1.0.0".to_string());
        root_children.insert("b".to_string(), "b@1.0.0".to_string());
        graph.insert(
            "root@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "root@1.0.0:sha512-root".to_string(),
                children: root_children,
            },
        );
        let mut a_children = HashMap::new();
        a_children.insert("c".to_string(), "c@1.0.0".to_string());
        graph.insert(
            "a@1.0.0".to_string(),
            DepsGraphNode { full_pkg_id: "a@1.0.0:sha512-a".to_string(), children: a_children },
        );
        let mut b_children = HashMap::new();
        b_children.insert("c".to_string(), "c@1.0.0".to_string());
        graph.insert(
            "b@1.0.0".to_string(),
            DepsGraphNode { full_pkg_id: "b@1.0.0:sha512-b".to_string(), children: b_children },
        );
        graph.insert(
            "c@1.0.0".to_string(),
            DepsGraphNode { full_pkg_id: "c@1.0.0:sha512-c".to_string(), children: HashMap::new() },
        );
        let mut cache = HashMap::new();
        let result = calc_dep_state(
            &graph,
            &mut cache,
            &"root@1.0.0".to_string(),
            &CalcDepStateOptions {
                engine_name: "darwin;arm64;node20",
                patch_file_hash: None,
                include_dep_graph_hash: true,
            },
        );
        // root + a + b + c = 4 cache entries.
        assert_eq!(cache.len(), 4, "expected 4 cache entries for diamond, got {cache:#?}");
        assert!(result.contains(";deps="), "result must include deps section: {result:?}");
    }

    /// Cycle: a depends on b, b depends on a. The walk must
    /// terminate (parents-set short-circuit) and produce a stable
    /// hash. Mirrors upstream's `if (!parents.has(node.fullPkgId))`
    /// guard at line 66.
    #[test]
    fn cyclic_graph_terminates_and_is_stable() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        let mut a_children = HashMap::new();
        a_children.insert("b".to_string(), "b@1.0.0".to_string());
        graph.insert(
            "a@1.0.0".to_string(),
            DepsGraphNode { full_pkg_id: "a@1.0.0:sha512-a".to_string(), children: a_children },
        );
        let mut b_children = HashMap::new();
        b_children.insert("a".to_string(), "a@1.0.0".to_string());
        graph.insert(
            "b@1.0.0".to_string(),
            DepsGraphNode { full_pkg_id: "b@1.0.0:sha512-b".to_string(), children: b_children },
        );
        let mut cache = HashMap::new();
        let opts = CalcDepStateOptions {
            engine_name: "darwin;arm64;node20",
            patch_file_hash: None,
            include_dep_graph_hash: true,
        };
        let h1 = calc_dep_state(&graph, &mut cache, &"a@1.0.0".to_string(), &opts);
        let h2 = calc_dep_state(&graph, &mut cache, &"a@1.0.0".to_string(), &opts);
        assert_eq!(h1, h2);
    }

    /// Both patch and dep graph hashes append in upstream's order:
    /// `<engine>;deps=<h>;patch=<h>`. Mirrors index.js:36-42.
    #[test]
    fn dep_graph_and_patch_concatenate_in_upstream_order() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        graph.insert(
            "x@1.0.0".to_string(),
            DepsGraphNode { full_pkg_id: "x@1.0.0:sha512-x".to_string(), children: HashMap::new() },
        );
        let mut cache = HashMap::new();
        let result = calc_dep_state(
            &graph,
            &mut cache,
            &"x@1.0.0".to_string(),
            &CalcDepStateOptions {
                engine_name: "darwin;arm64;node20",
                patch_file_hash: Some("patchhex"),
                include_dep_graph_hash: true,
            },
        );
        let deps_pos = result.find(";deps=").expect("deps section present");
        let patch_pos = result.find(";patch=").expect("patch section present");
        assert!(deps_pos < patch_pos, "deps must come before patch in {result:?}");
    }

    /// Self-hit: a snapshot listed in `built_dep_paths` returns
    /// `true` and caches `true` without looking at its children.
    #[test]
    fn transitively_requires_build_self_in_built_set() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        graph.insert(
            "builder@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "builder@1.0.0:sha512-b".to_string(),
                children: HashMap::new(),
            },
        );
        let built: std::collections::HashSet<String> =
            ["builder@1.0.0".to_string()].into_iter().collect();
        let mut cache = HashMap::new();
        let mut parents = std::collections::HashSet::new();
        assert!(transitively_requires_build(
            &graph,
            &built,
            &mut cache,
            &"builder@1.0.0".to_string(),
            &mut parents
        ));
        assert_eq!(cache.get("builder@1.0.0"), Some(&true));
    }

    /// Transitive: a snapshot whose child is a builder returns
    /// `true` and caches `true` at every level walked.
    #[test]
    fn transitively_requires_build_walks_to_descendant_builder() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        let mut root_children = HashMap::new();
        root_children.insert("dep".to_string(), "builder@1.0.0".to_string());
        graph.insert(
            "root@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "root@1.0.0:sha512-r".to_string(),
                children: root_children,
            },
        );
        graph.insert(
            "builder@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "builder@1.0.0:sha512-b".to_string(),
                children: HashMap::new(),
            },
        );
        let built: std::collections::HashSet<String> =
            ["builder@1.0.0".to_string()].into_iter().collect();
        let mut cache = HashMap::new();
        let mut parents = std::collections::HashSet::new();
        assert!(transitively_requires_build(
            &graph,
            &built,
            &mut cache,
            &"root@1.0.0".to_string(),
            &mut parents
        ));
        assert_eq!(cache.get("root@1.0.0"), Some(&true));
        assert_eq!(cache.get("builder@1.0.0"), Some(&true));
    }

    /// Unrelated: a snapshot whose subtree contains no builder
    /// returns `false` and caches `false` at every visited node.
    #[test]
    fn transitively_requires_build_returns_false_for_unrelated_tree() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        let mut root_children = HashMap::new();
        root_children.insert("dep".to_string(), "leaf@1.0.0".to_string());
        graph.insert(
            "root@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "root@1.0.0:sha512-r".to_string(),
                children: root_children,
            },
        );
        graph.insert(
            "leaf@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "leaf@1.0.0:sha512-l".to_string(),
                children: HashMap::new(),
            },
        );
        let built: std::collections::HashSet<String> =
            ["builder@9.9.9".to_string()].into_iter().collect();
        let mut cache = HashMap::new();
        let mut parents = std::collections::HashSet::new();
        assert!(!transitively_requires_build(
            &graph,
            &built,
            &mut cache,
            &"root@1.0.0".to_string(),
            &mut parents
        ));
        assert_eq!(cache.get("root@1.0.0"), Some(&false));
        assert_eq!(cache.get("leaf@1.0.0"), Some(&false));
    }

    /// Missing node: returns `false` and caches `false`. Mirrors
    /// upstream's `if (!node) { cache[depPath] = false; return false }`
    /// branch — the install-time graph build can drop entries for
    /// resolutions the linker rejects later, so the walker must not
    /// panic on missing keys.
    #[test]
    fn transitively_requires_build_caches_false_for_missing_node() {
        let graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        let built: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut cache = HashMap::new();
        let mut parents = std::collections::HashSet::new();
        assert!(!transitively_requires_build(
            &graph,
            &built,
            &mut cache,
            &"ghost@1.0.0".to_string(),
            &mut parents
        ));
        assert_eq!(cache.get("ghost@1.0.0"), Some(&false));
    }

    /// Cycle: a → b → a with no builder anywhere. The walk must
    /// terminate and return `false`. Cache state on `a` is `false`
    /// (the post-loop write), `b` is also `false`. The cycle short
    /// circuit at upstream's `if (parents.has(depPath)) return false`
    /// fires once but doesn't taint the eventual cache write at
    /// the frame that owns `a`.
    #[test]
    fn transitively_requires_build_cycle_terminates() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        let mut a_children = HashMap::new();
        a_children.insert("b".to_string(), "b@1.0.0".to_string());
        graph.insert(
            "a@1.0.0".to_string(),
            DepsGraphNode { full_pkg_id: "a@1.0.0:sha512-a".to_string(), children: a_children },
        );
        let mut b_children = HashMap::new();
        b_children.insert("a".to_string(), "a@1.0.0".to_string());
        graph.insert(
            "b@1.0.0".to_string(),
            DepsGraphNode { full_pkg_id: "b@1.0.0:sha512-b".to_string(), children: b_children },
        );
        let built: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut cache = HashMap::new();
        let mut parents = std::collections::HashSet::new();
        assert!(!transitively_requires_build(
            &graph,
            &built,
            &mut cache,
            &"a@1.0.0".to_string(),
            &mut parents
        ));
        assert_eq!(cache.get("a@1.0.0"), Some(&false));
        assert_eq!(cache.get("b@1.0.0"), Some(&false));
        assert!(parents.is_empty(), "parents set must be restored to empty");
    }

    /// Cycle with builder elsewhere in the tree: `a → b → a` plus
    /// `a → builder`. The cycle must not short-circuit `a`'s overall
    /// answer to `false` — the sibling builder visit still drives
    /// `a` to `true`. Verifies that the no-cache-on-cycle behaviour
    /// is what makes this work.
    #[test]
    fn transitively_requires_build_cycle_does_not_mask_sibling_builder() {
        let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
        // Use a BTreeMap-style two-key insert so child iteration
        // order can take either path; both must yield `true`.
        let mut a_children = HashMap::new();
        a_children.insert("b".to_string(), "b@1.0.0".to_string());
        a_children.insert("c".to_string(), "builder@1.0.0".to_string());
        graph.insert(
            "a@1.0.0".to_string(),
            DepsGraphNode { full_pkg_id: "a@1.0.0:sha512-a".to_string(), children: a_children },
        );
        let mut b_children = HashMap::new();
        b_children.insert("a".to_string(), "a@1.0.0".to_string());
        graph.insert(
            "b@1.0.0".to_string(),
            DepsGraphNode { full_pkg_id: "b@1.0.0:sha512-b".to_string(), children: b_children },
        );
        graph.insert(
            "builder@1.0.0".to_string(),
            DepsGraphNode {
                full_pkg_id: "builder@1.0.0:sha512-x".to_string(),
                children: HashMap::new(),
            },
        );
        let built: std::collections::HashSet<String> =
            ["builder@1.0.0".to_string()].into_iter().collect();
        let mut cache = HashMap::new();
        let mut parents = std::collections::HashSet::new();
        assert!(transitively_requires_build(
            &graph,
            &built,
            &mut cache,
            &"a@1.0.0".to_string(),
            &mut parents
        ));
    }
}
