use super::{CalcDepStateOptions, DepsGraphNode, calc_dep_state, transitively_requires_build};
use pretty_assertions::assert_eq;
use std::collections::HashMap;

/// Engine-only key (no dep graph, no patch). Pure prefix path
/// for the cheapest cache lookup. Mirrors the "`include_dep_graph_hash`:
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
        DepsGraphNode { full_pkg_id: "leaf@1.0.0:sha512-x".to_string(), children: HashMap::new() },
    );
    let mut cache = HashMap::new();
    let opts = CalcDepStateOptions {
        engine_name: "darwin;arm64;node20",
        patch_file_hash: None,
        include_dep_graph_hash: true,
    };
    let first = calc_dep_state(&graph, &mut cache, &"leaf@1.0.0".to_string(), &opts);
    let second = calc_dep_state(&graph, &mut cache, &"leaf@1.0.0".to_string(), &opts);
    assert_eq!(first, second);
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
        std::iter::once("builder@1.0.0".to_string()).collect();
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
        DepsGraphNode { full_pkg_id: "root@1.0.0:sha512-r".to_string(), children: root_children },
    );
    graph.insert(
        "builder@1.0.0".to_string(),
        DepsGraphNode {
            full_pkg_id: "builder@1.0.0:sha512-b".to_string(),
            children: HashMap::new(),
        },
    );
    let built: std::collections::HashSet<String> =
        std::iter::once("builder@1.0.0".to_string()).collect();
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
        DepsGraphNode { full_pkg_id: "root@1.0.0:sha512-r".to_string(), children: root_children },
    );
    graph.insert(
        "leaf@1.0.0".to_string(),
        DepsGraphNode { full_pkg_id: "leaf@1.0.0:sha512-l".to_string(), children: HashMap::new() },
    );
    let built: std::collections::HashSet<String> =
        std::iter::once("builder@9.9.9".to_string()).collect();
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
        std::iter::once("builder@1.0.0".to_string()).collect();
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
