use super::{
    CalcDepStateOptions, DepsGraphNode, build_required_dep_paths, calc_dep_graph_hash,
    calc_dep_state, calc_dep_state_input_key,
};
use crate::hash_object;
use indexmap::IndexMap;
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

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
    assert_eq!(result, "darwin;arm64;node20;format=2");
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
    assert_eq!(result, "linux;x64;node22;format=2;patch=sha256-abc");
}

#[test]
fn dep_graph_hash_for_leaf_uses_id_and_empty_deps() {
    let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
    graph.insert(
        "leaf@1.0.0".to_string(),
        DepsGraphNode {
            full_pkg_id: "leaf@1.0.0:sha512-leaf".to_string(),
            children: IndexMap::new(),
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
    assert!(
        parts.len() == 5,
        "expected `<plat>;<arch>;node<n>;format=2;deps=<hash>`, got {result:?}",
    );
    assert!(parts[4].starts_with("deps="), "fifth segment must be `deps=...`: {result:?}");
    assert!(parts[4][5..].len() >= 40, "hash payload must be non-trivial: {result:?}");
}

#[test]
fn cache_makes_repeat_calls_byte_equal() {
    let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
    graph.insert(
        "leaf@1.0.0".to_string(),
        DepsGraphNode { full_pkg_id: "leaf@1.0.0:sha512-x".to_string(), children: IndexMap::new() },
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
    let mut root_children = IndexMap::new();
    root_children.insert("a".to_string(), "a@1.0.0".to_string());
    root_children.insert("b".to_string(), "b@1.0.0".to_string());
    graph.insert(
        "root@1.0.0".to_string(),
        DepsGraphNode {
            full_pkg_id: "root@1.0.0:sha512-root".to_string(),
            children: root_children,
        },
    );
    let mut a_children = IndexMap::new();
    a_children.insert("c".to_string(), "c@1.0.0".to_string());
    graph.insert(
        "a@1.0.0".to_string(),
        DepsGraphNode { full_pkg_id: "a@1.0.0:sha512-a".to_string(), children: a_children },
    );
    let mut b_children = IndexMap::new();
    b_children.insert("c".to_string(), "c@1.0.0".to_string());
    graph.insert(
        "b@1.0.0".to_string(),
        DepsGraphNode { full_pkg_id: "b@1.0.0:sha512-b".to_string(), children: b_children },
    );
    graph.insert(
        "c@1.0.0".to_string(),
        DepsGraphNode { full_pkg_id: "c@1.0.0:sha512-c".to_string(), children: IndexMap::new() },
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
    let mut a_children = IndexMap::new();
    a_children.insert("b".to_string(), "b@1.0.0".to_string());
    graph.insert(
        "a@1.0.0".to_string(),
        DepsGraphNode { full_pkg_id: "a@1.0.0:sha512-a".to_string(), children: a_children },
    );
    let mut b_children = IndexMap::new();
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
        DepsGraphNode { full_pkg_id: "x@1.0.0:sha512-x".to_string(), children: IndexMap::new() },
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
fn shared_input_key_excludes_engine_and_includes_patch() {
    let mut graph: HashMap<String, DepsGraphNode<String>> = HashMap::new();
    graph.insert(
        "x@1.0.0".to_string(),
        DepsGraphNode { full_pkg_id: "x@1.0.0:sha512-x".to_string(), children: IndexMap::new() },
    );
    let key = calc_dep_state_input_key(&graph, &"x@1.0.0".to_string(), Some("patchhex"));
    let deps_hash = hash_object(&serde_json::json!({
        "id": "x@1.0.0:sha512-x",
        "deps": {},
    }));
    assert_eq!(key, format!("dependency-side-effects:v1:deps={deps_hash};patch=patchhex"));
}

#[test]
#[should_panic(expected = "input-key root is not present")]
fn shared_input_key_rejects_a_missing_root() {
    calc_dep_state_input_key(
        &HashMap::<String, DepsGraphNode<String>>::new(),
        &"missing@1.0.0".to_string(),
        None,
    );
}

#[test]
fn shared_input_keys_isolate_cyclic_roots() {
    let mut graph = HashMap::new();
    graph.insert(
        "a@1.0.0".to_string(),
        DepsGraphNode {
            full_pkg_id: "a@1.0.0:sha512-a".to_string(),
            children: IndexMap::from([("b".to_string(), "b@1.0.0".to_string())]),
        },
    );
    graph.insert(
        "b@1.0.0".to_string(),
        DepsGraphNode {
            full_pkg_id: "b@1.0.0:sha512-b".to_string(),
            children: IndexMap::from([("a".to_string(), "a@1.0.0".to_string())]),
        },
    );

    let label_a = hash_object(&serde_json::json!({ "id": "a@1.0.0:sha512-a", "deps": {} }));
    let label_b = hash_object(&serde_json::json!({ "id": "b@1.0.0:sha512-b", "deps": {} }));
    let mut shape = serde_json::Map::new();
    shape.insert(label_a.clone(), serde_json::json!({ "b": label_b }));
    shape.insert(label_b.clone(), serde_json::json!({ "a": label_a }));
    let component = hash_object(&serde_json::Value::Object(shape));
    let root_a = hash_object(&serde_json::json!({ "member": label_a, "component": component }));
    let root_b = hash_object(&serde_json::json!({ "member": label_b, "component": component }));

    assert_eq!(
        calc_dep_state_input_key(&graph, &"a@1.0.0".to_string(), None),
        format!("dependency-side-effects:v1:deps={root_a}"),
    );
    assert_eq!(
        calc_dep_state_input_key(&graph, &"b@1.0.0".to_string(), None),
        format!("dependency-side-effects:v1:deps={root_b}"),
    );
}

#[test]
fn build_required_dep_paths_reaches_every_parent_across_a_cycle() {
    let graph = HashMap::from([
        (
            "a".to_string(),
            DepsGraphNode {
                full_pkg_id: "a".to_string(),
                children: IndexMap::from([
                    ("b".to_string(), "b".to_string()),
                    ("builder".to_string(), "builder".to_string()),
                ]),
            },
        ),
        (
            "b".to_string(),
            DepsGraphNode {
                full_pkg_id: "b".to_string(),
                children: IndexMap::from([("a".to_string(), "a".to_string())]),
            },
        ),
        (
            "builder".to_string(),
            DepsGraphNode { full_pkg_id: "builder".to_string(), children: IndexMap::new() },
        ),
        (
            "unrelated".to_string(),
            DepsGraphNode { full_pkg_id: "unrelated".to_string(), children: IndexMap::new() },
        ),
    ]);

    assert_eq!(
        build_required_dep_paths(&graph, &HashSet::from(["builder".to_string()])),
        HashSet::from(["a".to_string(), "b".to_string(), "builder".to_string()]),
    );
}

#[test]
fn build_required_dep_paths_handles_empty_and_missing_builders() {
    let graph = HashMap::from([(
        "root".to_string(),
        DepsGraphNode {
            full_pkg_id: "root".to_string(),
            children: IndexMap::from([("missing".to_string(), "missing".to_string())]),
        },
    )]);

    assert!(build_required_dep_paths(&graph, &HashSet::new()).is_empty());
    assert_eq!(
        build_required_dep_paths(&graph, &HashSet::from(["missing".to_string()])),
        HashSet::from(["root".to_string(), "missing".to_string()]),
    );
}

fn node(id: &str, children: &[(&str, &str)]) -> DepsGraphNode<String> {
    DepsGraphNode {
        full_pkg_id: format!("{id}@1.0.0:sha512-{id}"),
        children: children
            .iter()
            .map(|(alias, key)| ((*alias).to_string(), (*key).to_string()))
            .collect(),
    }
}

fn graph_of<const LEN: usize>(
    nodes: [(&str, DepsGraphNode<String>); LEN],
) -> HashMap<String, DepsGraphNode<String>> {
    nodes
        .into_iter()
        .map(|(key, node)| (key.to_string(), node))
        .collect()
}

fn graph_hash(graph: &HashMap<String, DepsGraphNode<String>>, key: &str) -> String {
    calc_dep_graph_hash(graph, &mut HashMap::new(), &key.to_string())
}

/// `a` sits in two cycles at once — `a` ↔ `b` and `a` ↔ `m`.
fn two_cycle_graph() -> HashMap<String, DepsGraphNode<String>> {
    graph_of([
        ("a", node("a", &[("m", "m"), ("z", "z"), ("b", "b")])),
        ("b", node("b", &[("a", "a")])),
        ("m", node("m", &[("a", "a")])),
        ("z", node("z", &[])),
    ])
}

fn dep_states(
    graph: &HashMap<String, DepsGraphNode<String>>,
    cache: &mut HashMap<String, String>,
    query_order: &[&str],
) -> Vec<(String, String)> {
    let opts = CalcDepStateOptions {
        engine_name: "darwin;arm64;node20",
        patch_file_hash: None,
        include_dep_graph_hash: true,
    };
    let mut states: Vec<(String, String)> = query_order
        .iter()
        .map(|key| ((*key).to_string(), calc_dep_state(graph, cache, &(*key).to_string(), &opts)))
        .collect();
    states.sort();
    states
}

#[test]
fn cyclic_dep_states_do_not_depend_on_query_order() {
    let graph = two_cycle_graph();
    let forward = dep_states(&graph, &mut HashMap::new(), &["a", "b", "m", "z"]);
    let backward = dep_states(&graph, &mut HashMap::new(), &["z", "m", "b", "a"]);
    assert_eq!(forward, backward);
    let fresh_per_query: Vec<(String, String)> = ["a", "b", "m", "z"]
        .into_iter()
        .flat_map(|key| dep_states(&graph, &mut HashMap::new(), &[key]))
        .collect();
    assert_eq!(forward, fresh_per_query);
}

/// [pnpm/pnpm#17466](https://github.com/pnpm/pnpm/issues/17466): `core`
/// and `ws` depend on each other, so a change below `core` has to move
/// `ws` too, whichever of them the walk enters the cycle from.
#[test]
fn every_cycle_member_covers_what_the_cycle_reaches() {
    let with_leaf = |leaf_id: &str| {
        graph_of([
            ("core", node("core", &[("ws", "ws"), ("leaf", "leaf")])),
            ("ws", node("ws", &[("core", "core")])),
            ("leaf", node(leaf_id, &[])),
        ])
    };
    let before = with_leaf("leaf-1");
    let after = with_leaf("leaf-2");
    for key in ["core", "ws"] {
        assert_ne!(graph_hash(&before, key), graph_hash(&after, key), "{key} kept its digest");
    }
    let mut ws_first = HashMap::new();
    calc_dep_graph_hash(&before, &mut ws_first, &"ws".to_string());
    assert_eq!(ws_first["core"], graph_hash(&before, "core"));
}

#[test]
fn a_self_dependency_covers_its_other_dependencies() {
    let with_leaf = |leaf_id: &str| {
        graph_of([
            ("self", node("self", &[("self", "self"), ("leaf", "leaf")])),
            ("leaf", node(leaf_id, &[])),
        ])
    };
    assert_ne!(graph_hash(&with_leaf("leaf-1"), "self"), graph_hash(&with_leaf("leaf-2"), "self"));
}

/// `a1` and `a2` are one package in two peer contexts inside the same
/// cycle. Only `a1` reaches `leaf`, two steps into the cycle, so they
/// must not share a slot. The digest is pinned in pnpm's own tests too,
/// since both CLIs share one store.
#[test]
fn cycle_members_with_one_package_id_split_by_what_they_reach() {
    let graph = graph_of([
        ("a1", node("a", &[("b", "b1")])),
        ("a2", node("a", &[("b", "b2")])),
        ("b1", node("b", &[("a", "a2"), ("leaf", "leaf")])),
        ("b2", node("b", &[("a", "a1")])),
        ("leaf", node("leaf", &[])),
    ]);
    assert_eq!(graph_hash(&graph, "a1"), "OP/WBenoO6ds9rjouLr7e1U1KKHcnH5ZP6fvD1vWIjs=");
    assert_ne!(graph_hash(&graph, "a1"), graph_hash(&graph, "a2"));
    assert_ne!(graph_hash(&graph, "b1"), graph_hash(&graph, "b2"));
}

/// Unrolled, `a1` and `a2` link identical trees, so one slot serves both.
#[test]
fn cycle_members_with_identical_trees_share_a_digest() {
    let graph = graph_of([
        ("a1", node("a", &[("b", "b1")])),
        ("b1", node("b", &[("a", "a2")])),
        ("a2", node("a", &[("b", "b2")])),
        ("b2", node("b", &[("a", "a1")])),
    ]);
    assert_eq!(graph_hash(&graph, "a1"), graph_hash(&graph, "a2"));
    assert_eq!(graph_hash(&graph, "b1"), graph_hash(&graph, "b2"));
    assert_ne!(graph_hash(&graph, "a1"), graph_hash(&graph, "b1"));
}

/// A package reached twice on one path without a cycle in the graph is
/// hashed in full each time.
#[test]
fn a_repeated_package_id_without_a_cycle_is_not_truncated() {
    let with_leaf = |leaf_id: &str| {
        graph_of([
            ("a1", node("a", &[("b", "b")])),
            ("b", node("b", &[("a", "a2")])),
            ("a2", node("a", &[("leaf", "leaf")])),
            ("leaf", node(leaf_id, &[])),
        ])
    };
    assert_ne!(graph_hash(&with_leaf("leaf-1"), "a1"), graph_hash(&with_leaf("leaf-2"), "a1"));
}

/// The digest is a store-layout contract: it names the directory a
/// global-virtual-store slot lives in, so a change to the bytes fed to
/// SHA-256 relocates every package in every store.
/// `calc_dep_graph_hash` writes those bytes itself instead of building
/// the `serde_json` value [`hash_object`] would serialize, and only
/// this test holds the two to the same output.
///
/// The fixture covers what the hand-written serializer has to get
/// right: aliases in an order the sort has to correct, a child the
/// graph has no node for, and a node with no children at all.
#[test]
fn dep_graph_hash_matches_the_object_hash_of_the_value_it_models() {
    let graph: HashMap<String, DepsGraphNode<String>> = HashMap::from([
        ("root".to_string(), node("root", &[("z", "leaf"), ("a", "mid"), ("m", "absent")])),
        ("mid".to_string(), node("mid", &[("leaf", "leaf")])),
        ("leaf".to_string(), node("leaf", &[])),
    ]);

    let mut cache = HashMap::new();
    for dep_path in ["root", "mid", "leaf"] {
        let dep_path = dep_path.to_string();
        let actual = calc_dep_graph_hash(&graph, &mut cache, &dep_path);
        let node = &graph[&dep_path];
        let mut deps = serde_json::Map::new();
        for (alias, child_key) in &node.children {
            let child = calc_dep_graph_hash(&graph, &mut cache, child_key);
            deps.insert(alias.clone(), serde_json::Value::String(child));
        }
        let expected = hash_object(&serde_json::json!({
            "id": node.full_pkg_id.clone(),
            "deps": serde_json::Value::Object(deps),
        }));
        assert_eq!(actual, expected, "dep-graph hash diverged for {dep_path:?}");
    }
}
