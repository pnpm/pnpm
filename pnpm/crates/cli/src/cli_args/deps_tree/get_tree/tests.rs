//! Ports of the upstream `getTree` tests
//! (deps/inspection/tree-builder/test/getTree.test.ts).

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt::Write,
    path::Path,
};

use pacquet_lockfile::Lockfile;
use pacquet_modules_yaml::IncludedDependencies;
use pretty_assertions::assert_eq;

use super::{GetTreeOptions, MaterializationCache, MaxDepth, get_tree};
use crate::cli_args::deps_tree::{
    DependencyNode, TreeNodeId,
    graph::{BuildGraphOptions, DependencyGraph, build_dependency_graph},
    pkg_info::PkgInfoEnv,
    search::{SearchMatch, Searcher},
};

const MOCK_INTEGRITY: &str = "sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==";

/// Counterpart of the upstream test helper `generateMockCurrentPackages`:
/// every package gets version `1.0.0`, and every dependency name that is
/// mentioned but not declared gets its own empty entry.
fn mock_packages_yaml(packages: &[(&str, &[&str])]) -> String {
    let mut names: BTreeSet<&str> = packages.iter().map(|(name, _)| *name).collect();
    names.extend(packages.iter().flat_map(|(_, deps)| deps.iter().copied()));
    let deps_of: HashMap<&str, &[&str]> = packages.iter().copied().collect();

    let mut yaml = String::from("packages:\n");
    for name in &names {
        writeln!(yaml, "  {name}@1.0.0:\n    resolution: {{integrity: {MOCK_INTEGRITY}}}").unwrap();
    }
    yaml.push_str("\nsnapshots:\n");
    for name in &names {
        match deps_of.get(name).copied().unwrap_or_default() {
            [] => writeln!(yaml, "  {name}@1.0.0: {{}}").unwrap(),
            deps => {
                writeln!(yaml, "  {name}@1.0.0:\n    dependencies:").unwrap();
                for dep in deps {
                    writeln!(yaml, "      {dep}: 1.0.0").unwrap();
                }
            }
        }
    }
    yaml
}

fn load_lockfile(dir: &Path, yaml: &str) -> Lockfile {
    std::fs::write(dir.join("pnpm-lock.yaml"), yaml).unwrap();
    Lockfile::load_wanted_from_dir(dir).unwrap().unwrap()
}

fn mock_lockfile(dir: &Path, packages: &[(&str, &[&str])]) -> Lockfile {
    let yaml = format!(
        "lockfileVersion: '9.0'\n\nimporters:\n  .: {{}}\n\n{}",
        mock_packages_yaml(packages)
    );
    load_lockfile(dir, &yaml)
}

fn mock_env<'a>(dir: &Path, lockfile: &'a Lockfile) -> PkgInfoEnv<'a> {
    PkgInfoEnv {
        lockfile_dir: dir.to_path_buf(),
        modules_dir: dir.join("node_modules"),
        virtual_store_dir: dir.join("node_modules/.pnpm"),
        virtual_store_dir_max_length: 120,
        registries: HashMap::from([(
            "default".to_string(),
            "https://mock-registry-for-testing.example/".to_string(),
        )]),
        skipped: HashSet::new(),
        store_dir: None,
        current_lockfile: lockfile,
        wanted_lockfile: Some(lockfile),
        dep_types: HashMap::new(),
    }
}

/// The `include` filter the upstream tests pass: `{ optionalDependencies: false }`.
fn include_no_optional() -> IncludedDependencies {
    IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: false,
    }
}

fn graph_for(lockfile: &Lockfile, roots: &[TreeNodeId]) -> DependencyGraph {
    build_dependency_graph(
        roots,
        &BuildGraphOptions { lockfile, include: include_no_optional(), only_projects: false },
    )
}

fn pkg_root(name: &str) -> TreeNodeId {
    TreeNodeId::Package(format!("{name}@1.0.0").parse().unwrap())
}

/// Counterpart of the upstream test helper `getTreeWithGraph`.
fn tree_with_graph(
    env: &PkgInfoEnv<'_>,
    root: &TreeNodeId,
    max_depth: MaxDepth,
) -> Vec<DependencyNode> {
    let graph = graph_for(env.current_lockfile, std::slice::from_ref(root));
    let opts = GetTreeOptions {
        env,
        graph: &graph,
        exclude_peer_dependencies: false,
        only_projects: false,
        search: None,
        show_deduped_search_matches: false,
        rewrite_link_version_dir: env.lockfile_dir.clone(),
    };
    get_tree(&opts, &mut MaterializationCache::new(), root, max_depth, None)
}

fn tree_with_search(
    env: &PkgInfoEnv<'_>,
    root: &TreeNodeId,
    search: &Searcher,
    show_deduped_search_matches: bool,
) -> Vec<DependencyNode> {
    let graph = graph_for(env.current_lockfile, std::slice::from_ref(root));
    let opts = GetTreeOptions {
        env,
        graph: &graph,
        exclude_peer_dependencies: false,
        only_projects: false,
        search: Some(search),
        show_deduped_search_matches,
        rewrite_link_version_dir: env.lockfile_dir.clone(),
    };
    get_tree(&opts, &mut MaterializationCache::new(), root, MaxDepth::Unlimited, None)
}

fn query_searcher(query: &str) -> Searcher {
    Searcher::from_queries(&[query.to_string()]).unwrap()
}

/// A message-returning `--find-by` finder, pre-evaluated the way the CLI
/// seeds finder verdicts: keyed by `(alias, node)`.
fn finder_searcher(alias: &str, dep_path: &str, message: &str) -> Searcher {
    let mut searcher = Searcher::from_queries(&[]).unwrap();
    searcher.set_finder_results(HashMap::from([(
        (alias.to_string(), Some(TreeNodeId::Package(dep_path.parse().unwrap()))),
        SearchMatch::Message(message.to_string()),
    )]));
    searcher
}

/// Renders the alias nesting of a materialized tree, e.g. `b1(c1(d1)),b2,b3`.
/// Children come out sorted by alias, so the rendering is deterministic.
fn shape(nodes: &[DependencyNode]) -> String {
    nodes
        .iter()
        .map(|node| {
            if node.dependencies.is_empty() {
                node.alias.clone()
            } else {
                format!("{}({})", node.alias, shape(&node.dependencies))
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn find<'a>(nodes: &'a [DependencyNode], alias_path: &[&str]) -> &'a DependencyNode {
    let (first, rest) = alias_path.split_first().expect("alias path must be non-empty");
    let node = nodes
        .iter()
        .find(|node| node.alias == *first)
        .unwrap_or_else(|| panic!("no node with alias {first}"));
    if rest.is_empty() { node } else { find(&node.dependencies, rest) }
}

// Port of upstream's 'full test case to print when max depth is large' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn full_test_case_to_print_when_max_depth_is_large() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile =
        mock_lockfile(dir.path(), &[("a", &["b1", "b2", "b3"]), ("b1", &["c1"]), ("c1", &["d1"])]);
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &pkg_root("a"), MaxDepth::Finite(9999));

    assert_eq!(shape(&result), "b1(c1(d1)),b2,b3");
}

// Port of upstream's 'no result when current depth exceeds max depth' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn no_result_when_current_depth_exceeds_max_depth() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile =
        mock_lockfile(dir.path(), &[("a", &["b1", "b2", "b3"]), ("b1", &["c1"]), ("c1", &["d1"])]);
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &pkg_root("a"), MaxDepth::Finite(0));

    assert_eq!(shape(&result), "");
}

// Port of upstream's 'max depth of 1 to print flat dependencies' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn max_depth_of_1_to_print_flat_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile =
        mock_lockfile(dir.path(), &[("a", &["b1", "b2", "b3"]), ("b1", &["c1"]), ("c1", &["d1"])]);
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &pkg_root("a"), MaxDepth::Finite(1));

    assert_eq!(shape(&result), "b1,b2,b3");
}

// Port of upstream's 'max depth of 2 to print a1 -> b1 -> c1, but not d1' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn max_depth_of_2_to_print_a1_to_b1_to_c1_but_not_d1() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile =
        mock_lockfile(dir.path(), &[("a", &["b1", "b2", "b3"]), ("b1", &["c1"]), ("c1", &["d1"])]);
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &pkg_root("a"), MaxDepth::Finite(2));

    assert_eq!(shape(&result), "b1(c1),b2,b3");
}

// Port of upstream's 'revisiting package at lower depth prints dependencies not previously printed' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn revisiting_package_at_lower_depth_prints_dependencies_not_previously_printed() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(
        dir.path(),
        &[
            ("root", &["glob"]),
            ("glob", &["inflight", "once"]),
            ("inflight", &["once"]),
            ("once", &["wrappy"]),
        ],
    );
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &pkg_root("root"), MaxDepth::Finite(3));

    assert_eq!(shape(&result), "glob(inflight(once),once(wrappy))");
}

// Port of upstream's 'revisiting package at higher depth does not print extra dependencies' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn revisiting_package_at_higher_depth_does_not_print_extra_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(
        dir.path(),
        &[("root", &["a"]), ("a", &["b", "d"]), ("b", &["c"]), ("d", &["b"])],
    );
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &pkg_root("root"), MaxDepth::Finite(3));

    assert_eq!(shape(&result), "a(b(c),d(b))");
}

// Port of upstream's 'height < requestedDepth' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn height_less_than_requested_depth() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile =
        mock_lockfile(dir.path(), &[("root", &["a", "b"]), ("a", &["b"]), ("b", &["c"])]);
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &pkg_root("root"), MaxDepth::Finite(4));

    assert_eq!(shape(&result), "a(b(c)),b(c)");
}

// Port of upstream's 'height === requestedDepth' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn height_equals_requested_depth() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(
        dir.path(),
        &[("root", &["a", "c"]), ("a", &["b"]), ("c", &["d"]), ("d", &["a"])],
    );
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &pkg_root("root"), MaxDepth::Finite(4));

    assert_eq!(shape(&result), "a(b),c(d(a(b)))");
}

// Port of upstream's 'height === requestedDepth + 1' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn height_equals_requested_depth_plus_1() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(
        dir.path(),
        &[("root", &["a", "c"]), ("a", &["b"]), ("c", &["d"]), ("d", &["a"])],
    );
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &pkg_root("root"), MaxDepth::Finite(3));

    assert_eq!(shape(&result), "a(b),c(d(a))");
}

// Port of upstream's 'height > requestedDepth' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn height_greater_than_requested_depth() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(
        dir.path(),
        &[
            ("root", &["a", "e"]),
            ("a", &["b"]),
            ("b", &["c"]),
            ("c", &["d"]),
            ("e", &["f"]),
            ("f", &["g"]),
            ("g", &["a"]),
        ],
    );
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &pkg_root("root"), MaxDepth::Finite(5));

    assert_eq!(shape(&result), "a(b(c(d))),e(f(g(a(b))))");
}

// Port of upstream's 'marks back-edge as circular in a simple cycle' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn marks_back_edge_as_circular_in_a_simple_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(dir.path(), &[("root", &["a"]), ("a", &["b"]), ("b", &["a"])]);
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &pkg_root("root"), MaxDepth::Unlimited);

    assert_eq!(shape(&result), "a(b(a))");
    let back_edge = find(&result, &["a", "b", "a"]);
    dbg!(back_edge);
    assert!(back_edge.circular);
}

// Port of upstream's 'does not mark a node as circular when reached from a non-cyclic path' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn does_not_mark_a_node_as_circular_when_reached_from_a_non_cyclic_path() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(
        dir.path(),
        &[("root", &["a", "c"]), ("a", &["b"]), ("b", &["a"]), ("c", &["b"])],
    );
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &pkg_root("root"), MaxDepth::Unlimited);

    assert_eq!(shape(&result), "a(b(a)),c(b)");
    let back_edge = find(&result, &["a", "b", "a"]);
    let b_under_c = find(&result, &["c", "b"]);
    dbg!(back_edge, b_under_c);
    assert!(back_edge.circular);
    // b under c is deduped (already expanded under a), not circular.
    assert!(!b_under_c.circular);
    assert!(b_under_c.deduped);
}

// Port of upstream's 'link outside workspace appears as leaf node' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn link_outside_workspace_appears_as_leaf_node() {
    let dir = tempfile::tempdir().unwrap();
    let yaml = format!(
        "lockfileVersion: '9.0'

importers:
  .:
    dependencies:
      my-link:
        specifier: link:../external-pkg
        version: link:../external-pkg
      regular-dep:
        specifier: 1.0.0
        version: 1.0.0

{}",
        mock_packages_yaml(&[("regular-dep", &["transitive"])])
    );
    let lockfile = load_lockfile(dir.path(), &yaml);
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &TreeNodeId::Importer(".".to_string()), MaxDepth::Unlimited);

    assert_eq!(shape(&result), "my-link,regular-dep(transitive)");
    assert_eq!(find(&result, &["my-link"]).version, "link:../external-pkg");
}

// Port of upstream's 'link inside workspace resolves to importer and is traversed' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn link_inside_workspace_resolves_to_importer_and_is_traversed() {
    let dir = tempfile::tempdir().unwrap();
    let yaml = format!(
        "lockfileVersion: '9.0'

importers:
  .:
    dependencies:
      workspace-pkg:
        specifier: link:packages/workspace-pkg
        version: link:packages/workspace-pkg
  packages/workspace-pkg:
    dependencies:
      leaf:
        specifier: 1.0.0
        version: 1.0.0

{}",
        mock_packages_yaml(&[("leaf", &[])])
    );
    let lockfile = load_lockfile(dir.path(), &yaml);
    let env = mock_env(dir.path(), &lockfile);

    let result = tree_with_graph(&env, &TreeNodeId::Importer(".".to_string()), MaxDepth::Unlimited);

    assert_eq!(shape(&result), "workspace-pkg(leaf)");
    assert_eq!(find(&result, &["workspace-pkg"]).version, "link:packages/workspace-pkg");
}

// Port of upstream's 'deduped subtree containing a search match still appears in output' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn deduped_subtree_containing_a_search_match_still_appears_in_output() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(
        dir.path(),
        &[("root", &["a", "c"]), ("a", &["b"]), ("b", &["target"]), ("c", &["b"])],
    );
    let env = mock_env(dir.path(), &lockfile);
    let search = query_searcher("target");

    let result = tree_with_search(&env, &pkg_root("root"), &search, true);

    assert_eq!(shape(&result), "a(b(target)),c(b)");
    let matched = find(&result, &["a", "b", "target"]);
    let b_under_c = find(&result, &["c", "b"]);
    dbg!(matched, b_under_c);
    assert!(matched.searched);
    assert!(b_under_c.deduped);
    assert!(b_under_c.searched);
}

// Port of upstream's 'deduped subtree propagates string search messages to the deduped node' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn deduped_subtree_propagates_string_search_messages_to_the_deduped_node() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(
        dir.path(),
        &[("root", &["a", "c"]), ("a", &["b"]), ("b", &["target"]), ("c", &["b"])],
    );
    let env = mock_env(dir.path(), &lockfile);
    let search = finder_searcher("target", "target@1.0.0", "depends on target");

    let result = tree_with_search(&env, &pkg_root("root"), &search, true);

    assert_eq!(shape(&result), "a(b(target)),c(b)");
    let matched = find(&result, &["a", "b", "target"]);
    let b_under_c = find(&result, &["c", "b"]);
    dbg!(matched, b_under_c);
    assert!(matched.searched);
    assert_eq!(matched.search_message.as_deref(), Some("depends on target"));
    // The deduped b under c carries the search message from target.
    assert!(b_under_c.deduped);
    assert!(b_under_c.searched);
    assert_eq!(b_under_c.search_message.as_deref(), Some("depends on target"));
}

// Port of upstream's 'deduped subtree with search match is hidden by default' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn deduped_subtree_with_search_match_is_hidden_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(
        dir.path(),
        &[("root", &["a", "c"]), ("a", &["b"]), ("b", &["target"]), ("c", &["b"])],
    );
    let env = mock_env(dir.path(), &lockfile);
    let search = query_searcher("target");

    let result = tree_with_search(&env, &pkg_root("root"), &search, false);

    // Only a -> b -> target appears; c is excluded because its only child b
    // is deduped and does not directly match.
    assert_eq!(shape(&result), "a(b(target))");
}

// Port of upstream's 'deduped subtree without search match is excluded when search is active' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn deduped_subtree_without_search_match_is_excluded_when_search_is_active() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(
        dir.path(),
        &[("root", &["a", "c"]), ("a", &["b"]), ("b", &["leaf"]), ("c", &["b"])],
    );
    let env = mock_env(dir.path(), &lockfile);
    let search = query_searcher("target");

    let result = tree_with_search(&env, &pkg_root("root"), &search, false);

    assert_eq!(shape(&result), "");
}

// Port of upstream's 'graph includes nodes reachable from all specified root IDs' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn graph_includes_nodes_reachable_from_all_specified_root_ids() {
    let dir = tempfile::tempdir().unwrap();
    let yaml = format!(
        "lockfileVersion: '9.0'

importers:
  project-a:
    dependencies:
      a:
        specifier: 1.0.0
        version: 1.0.0
  project-b:
    dependencies:
      b:
        specifier: 1.0.0
        version: 1.0.0

{}",
        mock_packages_yaml(&[("a", &["shared"]), ("b", &["unique-to-b"]), ("shared", &["deep"])])
    );
    let lockfile = load_lockfile(dir.path(), &yaml);
    let root_a = TreeNodeId::Importer("project-a".to_string());
    let root_b = TreeNodeId::Importer("project-b".to_string());

    let multi = graph_for(&lockfile, &[root_a.clone(), root_b.clone()]);
    let graph_a = graph_for(&lockfile, std::slice::from_ref(&root_a));
    let graph_b = graph_for(&lockfile, std::slice::from_ref(&root_b));

    dbg!(multi.nodes.keys().collect::<Vec<_>>());
    for key in graph_a.nodes.keys().chain(graph_b.nodes.keys()) {
        assert!(multi.nodes.contains_key(key), "multi-root graph is missing {key:?}");
    }
    for name in ["unique-to-b", "shared", "deep"] {
        let id = pkg_root(name);
        assert!(multi.nodes.contains_key(&id), "multi-root graph is missing {id:?}");
    }
}

// Port of upstream's 'second getTree call for same node returns deduped children' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn second_get_tree_call_for_same_node_returns_deduped_children() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(dir.path(), &[("root", &["a"]), ("a", &["b"]), ("b", &["c"])]);
    let env = mock_env(dir.path(), &lockfile);
    let root = pkg_root("root");
    let graph = graph_for(&lockfile, std::slice::from_ref(&root));
    let opts = GetTreeOptions {
        env: &env,
        graph: &graph,
        exclude_peer_dependencies: false,
        only_projects: false,
        search: None,
        show_deduped_search_matches: false,
        rewrite_link_version_dir: dir.path().to_path_buf(),
    };
    let mut cache = MaterializationCache::new();

    let result1 = get_tree(&opts, &mut cache, &root, MaxDepth::Unlimited, None);
    assert_eq!(shape(&result1), "a(b(c))");

    let result2 = get_tree(&opts, &mut cache, &root, MaxDepth::Unlimited, None);
    assert_eq!(shape(&result2), "a");
    let deduped_a = &result2[0];
    dbg!(deduped_a);
    assert!(deduped_a.deduped);
    assert!(deduped_a.deduped_dependencies_count.unwrap() > 0);
}

// Port of upstream's 'deduped result preserves search match metadata' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn deduped_result_preserves_search_match_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(dir.path(), &[("root", &["a"]), ("a", &["target"])]);
    let env = mock_env(dir.path(), &lockfile);
    let root = pkg_root("root");
    let graph = graph_for(&lockfile, std::slice::from_ref(&root));
    let search = finder_searcher("target", "target@1.0.0", "found target");
    let opts = GetTreeOptions {
        env: &env,
        graph: &graph,
        exclude_peer_dependencies: false,
        only_projects: false,
        search: Some(&search),
        show_deduped_search_matches: true,
        rewrite_link_version_dir: dir.path().to_path_buf(),
    };
    let mut cache = MaterializationCache::new();

    let result1 = get_tree(&opts, &mut cache, &root, MaxDepth::Unlimited, None);
    assert_eq!(shape(&result1), "a(target)");
    let matched = find(&result1, &["a", "target"]);
    dbg!(matched);
    assert!(matched.searched);
    assert_eq!(matched.search_message.as_deref(), Some("found target"));

    // The second call dedupes a but carries the search metadata from the cache.
    let result2 = get_tree(&opts, &mut cache, &root, MaxDepth::Unlimited, None);
    assert_eq!(shape(&result2), "a");
    let deduped_a = &result2[0];
    dbg!(deduped_a);
    assert!(deduped_a.deduped);
    assert!(deduped_a.searched);
    assert_eq!(deduped_a.search_message.as_deref(), Some("found target"));
}

// Port of upstream's 'dedupedDependenciesCount correctly reflects subtree size' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn deduped_dependencies_count_correctly_reflects_subtree_size() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(dir.path(), &[("root", &["a"]), ("a", &["b", "c"])]);
    let env = mock_env(dir.path(), &lockfile);
    let root = pkg_root("root");
    let graph = graph_for(&lockfile, std::slice::from_ref(&root));
    let opts = GetTreeOptions {
        env: &env,
        graph: &graph,
        exclude_peer_dependencies: false,
        only_projects: false,
        search: None,
        show_deduped_search_matches: false,
        rewrite_link_version_dir: dir.path().to_path_buf(),
    };
    let mut cache = MaterializationCache::new();

    let result1 = get_tree(&opts, &mut cache, &root, MaxDepth::Unlimited, None);
    assert_eq!(shape(&result1), "a(b,c)");

    let result2 = get_tree(&opts, &mut cache, &root, MaxDepth::Unlimited, None);
    assert_eq!(shape(&result2), "a");
    let deduped_a = &result2[0];
    dbg!(deduped_a);
    assert!(deduped_a.deduped);
    // a's subtree had 2 nodes (b and c).
    assert_eq!(deduped_a.deduped_dependencies_count, Some(2));
}

// Port of upstream's 'different maxDepth values are cached independently' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn different_max_depth_values_are_cached_independently() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = mock_lockfile(dir.path(), &[("root", &["a"]), ("a", &["b"]), ("b", &["c"])]);
    let env = mock_env(dir.path(), &lockfile);
    let root = pkg_root("root");
    let graph = graph_for(&lockfile, std::slice::from_ref(&root));
    let opts = GetTreeOptions {
        env: &env,
        graph: &graph,
        exclude_peer_dependencies: false,
        only_projects: false,
        search: None,
        show_deduped_search_matches: false,
        rewrite_link_version_dir: dir.path().to_path_buf(),
    };
    let mut cache = MaterializationCache::new();

    // Depth 1 only shows a without children.
    let shallow = get_tree(&opts, &mut cache, &root, MaxDepth::Finite(1), None);
    assert_eq!(shape(&shallow), "a");

    // Unlimited depth shows the full tree, unaffected by the depth-1 cache.
    let deep = get_tree(&opts, &mut cache, &root, MaxDepth::Unlimited, None);
    assert_eq!(shape(&deep), "a(b(c))");
}

// Port of upstream's 'exclude peers' (deps/inspection/tree-builder/test/getTree.test.ts).
#[test]
fn exclude_peers() {
    let dir = tempfile::tempdir().unwrap();
    let yaml = format!(
        "lockfileVersion: '9.0'

importers:
  .: {{}}

packages:
  bar@1.0.0:
    resolution: {{integrity: {MOCK_INTEGRITY}}}
  foo@1.0.0:
    resolution: {{integrity: {MOCK_INTEGRITY}}}
    peerDependencies:
      peer1: ^1.0.0
      peer2: ^1.0.0
  peer1@1.0.0:
    resolution: {{integrity: {MOCK_INTEGRITY}}}
  peer2@1.0.0:
    resolution: {{integrity: {MOCK_INTEGRITY}}}
  qar@1.0.0:
    resolution: {{integrity: {MOCK_INTEGRITY}}}

snapshots:
  bar@1.0.0: {{}}
  foo@1.0.0:
    dependencies:
      peer1: 1.0.0
      peer2: 1.0.0
      qar: 1.0.0
  peer1@1.0.0:
    dependencies:
      bar: 1.0.0
  peer2@1.0.0: {{}}
  qar@1.0.0: {{}}
"
    );
    let lockfile = load_lockfile(dir.path(), &yaml);
    let env = mock_env(dir.path(), &lockfile);
    let root = pkg_root("foo");
    let graph = graph_for(&lockfile, std::slice::from_ref(&root));
    let opts = GetTreeOptions {
        env: &env,
        graph: &graph,
        exclude_peer_dependencies: true,
        only_projects: false,
        search: None,
        show_deduped_search_matches: false,
        rewrite_link_version_dir: dir.path().to_path_buf(),
    };

    let result =
        get_tree(&opts, &mut MaterializationCache::new(), &root, MaxDepth::Finite(9999), None);

    // peer1 stays because it has dependencies of its own; peer2 is excluded.
    assert_eq!(shape(&result), "peer1(bar),qar");
}
