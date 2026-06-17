use super::{CalcDepStateOptions, DepsGraphNode, calc_dep_state, transitively_requires_build};
use pretty_assertions::assert_eq;
use std::collections::HashMap;

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
    let parts: Vec<&str> = result.split(';').collect();
    assert!(parts.len() == 4, "expected `<plat>;<arch>;node<n>;deps=<hash>`, got {result:?}");
    assert!(parts[3].starts_with("deps="), "fourth segment must be `deps=...`: {result:?}");
    assert!(parts[3][5..].len() >= 40, "hash payload must be non-trivial: {result:?}");
}

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
    assert_eq!(cache.len(), 4, "expected 4 cache entries for diamond, got {cache:#?}");
    assert!(result.contains(";deps="), "result must include deps section: {result:?}");
}

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

/// The install-time graph build can drop entries for resolutions the
/// linker rejects later, so the walker must not panic on missing keys.
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

#[test]
fn transitively_requires_build_cycle_does_not_mask_sibling_builder() {
    let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
    // Two children so child iteration order can take either path.
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
