use super::{
    calc_global_virtual_store_path_with_subdeps, calc_graph_node_hash,
    calc_leaf_global_virtual_store_path, format_global_virtual_store_path,
};
use crate::dep_state::DepsGraphNode;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Scoped packages don't get the `@/` prefix — they already start
/// with `@<scope>/`. Unscoped packages do.
#[test]
fn format_prefixes_unscoped_with_at_slash() {
    assert_eq!(
        format_global_virtual_store_path("foo", "1.2.3", "deadbeef"),
        "@/foo/1.2.3/deadbeef",
    );
    assert_eq!(
        format_global_virtual_store_path("@scope/foo", "1.2.3", "deadbeef"),
        "@scope/foo/1.2.3/deadbeef",
    );
}

/// Two graphs with identical structure and `full_pkg_id`s produce
/// the same hash — same as upstream's design where the GVS path is
/// the deduplication key.
#[test]
fn identical_leaves_hash_identically() {
    let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
    graph.insert(
        "leaf@1.0.0".to_string(),
        DepsGraphNode { full_pkg_id: "leaf@1.0.0:sha512-x".to_string(), children: HashMap::new() },
    );
    let mut cache_a = HashMap::new();
    let mut cache_b = HashMap::new();
    let mut br_a = HashMap::new();
    let mut br_b = HashMap::new();
    let first = calc_graph_node_hash(
        &graph,
        &mut cache_a,
        &"leaf@1.0.0".to_string(),
        Some("darwin-arm64-node20"),
        None,
        &mut br_a,
    );
    let second = calc_graph_node_hash(
        &graph,
        &mut cache_b,
        &"leaf@1.0.0".to_string(),
        Some("darwin-arm64-node20"),
        None,
        &mut br_b,
    );
    assert_eq!(first, second, "deterministic for same input");
    assert_eq!(first.len(), 64, "sha256 hex digest is 64 chars");
}

/// Different engines produce different hashes when the engine
/// is included. Mirrors upstream's contribution of `ENGINE_NAME`
/// (vs `null`) to the hash payload at
/// `deps/graph-hasher/src/index.ts:140-146`.
#[test]
fn engine_string_changes_hash() {
    let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
    graph.insert(
        "leaf@1.0.0".to_string(),
        DepsGraphNode { full_pkg_id: "leaf@1.0.0:sha512-x".to_string(), children: HashMap::new() },
    );
    let mut cache = HashMap::new();
    let mut br = HashMap::new();
    let with_engine = calc_graph_node_hash(
        &graph,
        &mut cache,
        &"leaf@1.0.0".to_string(),
        Some("darwin-arm64-node20"),
        None,
        &mut br,
    );
    let mut cache_other = HashMap::new();
    let mut br_other = HashMap::new();
    let with_other_engine = calc_graph_node_hash(
        &graph,
        &mut cache_other,
        &"leaf@1.0.0".to_string(),
        Some("linux-x64-node22"),
        None,
        &mut br_other,
    );
    let mut cache_null = HashMap::new();
    let mut br_null = HashMap::new();
    let with_null = calc_graph_node_hash(
        &graph,
        &mut cache_null,
        &"leaf@1.0.0".to_string(),
        None,
        None,
        &mut br_null,
    );
    assert_ne!(with_engine, with_other_engine);
    assert_ne!(with_engine, with_null);
    assert_ne!(with_other_engine, with_null);
}

/// Engine-agnostic gating: when `built_dep_paths` excludes the
/// snapshot and its subtree, two different `engine` strings
/// hash to the *same* digest. This is the whole point of the
/// `transitivelyRequiresBuild` gating — pure-JS leaves survive
/// Node.js upgrades because their GVS hash drops the engine
/// contribution.
#[test]
fn engine_agnostic_when_subtree_has_no_builders() {
    let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
    graph.insert(
        "pure-js@1.0.0".to_string(),
        DepsGraphNode {
            full_pkg_id: "pure-js@1.0.0:sha512-x".to_string(),
            children: HashMap::new(),
        },
    );
    // builtDepPaths exists but doesn't contain pure-js. Gating fires.
    let built: HashSet<String> = std::iter::once("someone-else@1.0.0".to_string()).collect();
    let mut cache_a = HashMap::new();
    let mut br_a = HashMap::new();
    let darwin = calc_graph_node_hash(
        &graph,
        &mut cache_a,
        &"pure-js@1.0.0".to_string(),
        Some("darwin-arm64-node20"),
        Some(&built),
        &mut br_a,
    );
    let mut cache_b = HashMap::new();
    let mut br_b = HashMap::new();
    let linux = calc_graph_node_hash(
        &graph,
        &mut cache_b,
        &"pure-js@1.0.0".to_string(),
        Some("linux-x64-node22"),
        Some(&built),
        &mut br_b,
    );
    assert_eq!(
        darwin, linux,
        "pure-js subtree must hash engine-agnostically when gated by builtDepPaths",
    );
}

/// Gating's positive case: when the snapshot itself is in
/// `built_dep_paths`, the engine *is* included — two different
/// engines diverge. Symmetric with [`engine_agnostic_when_subtree_has_no_builders`].
#[test]
fn engine_included_when_self_in_built_set() {
    let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
    graph.insert(
        "native@1.0.0".to_string(),
        DepsGraphNode {
            full_pkg_id: "native@1.0.0:sha512-n".to_string(),
            children: HashMap::new(),
        },
    );
    let built: HashSet<String> = std::iter::once("native@1.0.0".to_string()).collect();
    let mut cache_a = HashMap::new();
    let mut br_a = HashMap::new();
    let darwin = calc_graph_node_hash(
        &graph,
        &mut cache_a,
        &"native@1.0.0".to_string(),
        Some("darwin-arm64-node20"),
        Some(&built),
        &mut br_a,
    );
    let mut cache_b = HashMap::new();
    let mut br_b = HashMap::new();
    let linux = calc_graph_node_hash(
        &graph,
        &mut cache_b,
        &"native@1.0.0".to_string(),
        Some("linux-x64-node22"),
        Some(&built),
        &mut br_b,
    );
    assert_ne!(darwin, linux, "builder must partition by engine string");
}

/// Gating's transitive case: a non-builder snapshot whose
/// child *is* a builder also includes the engine. Mirrors
/// upstream's recursion at index.ts:241-245.
#[test]
fn engine_included_for_ancestor_of_builder() {
    let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
    let mut root_children = HashMap::new();
    root_children.insert("dep".to_string(), "native@1.0.0".to_string());
    graph.insert(
        "root@1.0.0".to_string(),
        DepsGraphNode { full_pkg_id: "root@1.0.0:sha512-r".to_string(), children: root_children },
    );
    graph.insert(
        "native@1.0.0".to_string(),
        DepsGraphNode {
            full_pkg_id: "native@1.0.0:sha512-n".to_string(),
            children: HashMap::new(),
        },
    );
    let built: HashSet<String> = std::iter::once("native@1.0.0".to_string()).collect();
    let mut cache_a = HashMap::new();
    let mut br_a = HashMap::new();
    let darwin = calc_graph_node_hash(
        &graph,
        &mut cache_a,
        &"root@1.0.0".to_string(),
        Some("darwin-arm64-node20"),
        Some(&built),
        &mut br_a,
    );
    let mut cache_b = HashMap::new();
    let mut br_b = HashMap::new();
    let linux = calc_graph_node_hash(
        &graph,
        &mut cache_b,
        &"root@1.0.0".to_string(),
        Some("linux-x64-node22"),
        Some(&built),
        &mut br_b,
    );
    assert_ne!(darwin, linux, "ancestor of a builder must partition by engine string");
}

/// `built_dep_paths = None` reproduces the always-include-engine
/// behaviour: two different engines hash differently even for a
/// snapshot that no builder reaches. Mirrors upstream's
/// `builtDepPaths === undefined || ...` short-circuit at
/// index.ts:140.
#[test]
fn none_built_dep_paths_disables_gating() {
    let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
    graph.insert(
        "pure-js@1.0.0".to_string(),
        DepsGraphNode {
            full_pkg_id: "pure-js@1.0.0:sha512-x".to_string(),
            children: HashMap::new(),
        },
    );
    let mut cache_a = HashMap::new();
    let mut br_a = HashMap::new();
    let darwin = calc_graph_node_hash(
        &graph,
        &mut cache_a,
        &"pure-js@1.0.0".to_string(),
        Some("darwin-arm64-node20"),
        None,
        &mut br_a,
    );
    let mut cache_b = HashMap::new();
    let mut br_b = HashMap::new();
    let linux = calc_graph_node_hash(
        &graph,
        &mut cache_b,
        &"pure-js@1.0.0".to_string(),
        Some("linux-x64-node22"),
        None,
        &mut br_b,
    );
    assert_ne!(darwin, linux, "without builtDepPaths gating, engine is always part of the hash");
}

/// Two snapshots whose children differ (same `full_pkg_id`,
/// different deps) hash differently — the GVS path includes the
/// transitive dep contribution.
#[test]
fn different_children_change_hash() {
    let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
    graph.insert(
        "leaf@1.0.0".to_string(),
        DepsGraphNode { full_pkg_id: "leaf@1.0.0:sha512-x".to_string(), children: HashMap::new() },
    );
    let mut root_a_children = HashMap::new();
    root_a_children.insert("a".to_string(), "leaf@1.0.0".to_string());
    graph.insert(
        "root@1.0.0(a)".to_string(),
        DepsGraphNode { full_pkg_id: "root@1.0.0:sha512-r".to_string(), children: root_a_children },
    );
    graph.insert(
        "root@1.0.0(b)".to_string(),
        DepsGraphNode { full_pkg_id: "root@1.0.0:sha512-r".to_string(), children: HashMap::new() },
    );
    let mut cache_a = HashMap::new();
    let mut br_a = HashMap::new();
    let with_dep = calc_graph_node_hash(
        &graph,
        &mut cache_a,
        &"root@1.0.0(a)".to_string(),
        Some("darwin-arm64-node20"),
        None,
        &mut br_a,
    );
    let mut cache_b = HashMap::new();
    let mut br_b = HashMap::new();
    let without_dep = calc_graph_node_hash(
        &graph,
        &mut cache_b,
        &"root@1.0.0(b)".to_string(),
        Some("darwin-arm64-node20"),
        None,
        &mut br_b,
    );
    assert_ne!(with_dep, without_dep, "same root, different children must not collide on GVS hash");
}

/// The leaf hasher must produce the same digest as the general graph
/// hasher fed a single childless node with `engine = null`. This ties
/// `calc_leaf_global_virtual_store_path` to the already-parity-tested
/// `calc_graph_node_hash` recursion, so the leaf path stays byte-for-
/// byte identical to what pnpm computes.
#[test]
fn leaf_matches_single_node_graph_hash() {
    let full = "leaf@1.0.0:sha512-x";
    let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
    graph.insert(
        "leaf@1.0.0".to_string(),
        DepsGraphNode { full_pkg_id: full.to_string(), children: HashMap::new() },
    );
    let mut cache = HashMap::new();
    let mut br = HashMap::new();
    let digest =
        calc_graph_node_hash(&graph, &mut cache, &"leaf@1.0.0".to_string(), None, None, &mut br);
    assert_eq!(
        calc_leaf_global_virtual_store_path(full, "leaf", "1.0.0"),
        format_global_virtual_store_path("leaf", "1.0.0", &digest),
    );
}

/// With no subdeps, the subdeps hasher collapses to the leaf hasher:
/// an empty `deps` map hashes identically to `calcLeafGlobalVirtualStorePath`'s
/// `{ id, deps: {} }`.
#[test]
fn with_empty_subdeps_equals_leaf() {
    let full = "@scope/cfg@2.0.0:sha512-y";
    assert_eq!(
        calc_global_virtual_store_path_with_subdeps(full, "@scope/cfg", "2.0.0", &BTreeMap::new()),
        calc_leaf_global_virtual_store_path(full, "@scope/cfg", "2.0.0"),
    );
}

/// Adding an optional subdep, and changing its resolved id, must each
/// change the parent's GVS path — otherwise a subdep version swap would
/// silently reuse the parent's existing leaf directory.
#[test]
fn subdeps_partition_the_hash() {
    let parent = "cfg@1.0.0:sha512-p";
    let none =
        calc_global_virtual_store_path_with_subdeps(parent, "cfg", "1.0.0", &BTreeMap::new());

    let mut one = BTreeMap::new();
    one.insert("sub".to_string(), "sub@1.0.0:sha512-a".to_string());
    let with_sub = calc_global_virtual_store_path_with_subdeps(parent, "cfg", "1.0.0", &one);

    let mut other = BTreeMap::new();
    other.insert("sub".to_string(), "sub@1.0.1:sha512-b".to_string());
    let with_other_sub =
        calc_global_virtual_store_path_with_subdeps(parent, "cfg", "1.0.0", &other);

    assert_ne!(none, with_sub, "adding a subdep must change the parent hash");
    assert_ne!(with_sub, with_other_sub, "changing a subdep id must change the parent hash");
    assert_eq!(with_sub.matches('/').count(), 3, "path stays at <prefix>name/version/hash depth");
}
