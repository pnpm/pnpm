use pnpm_deps_inspection::dependents::{DepField, DependentNode, DependentsTree};
use pretty_assertions::assert_eq;

use super::render_dependents_ranges;

fn tree(
    version: &str,
    peers_suffix_hash: Option<&str>,
    dependents: Vec<DependentNode>,
) -> DependentsTree {
    let mut tree = DependentsTree::default();
    tree.name = "esbuild".to_string();
    tree.version = version.to_string();
    tree.peers_suffix_hash = peers_suffix_hash.map(str::to_string);
    tree.dependents = dependents;
    tree
}

fn dependent(name: &str, version: &str, requires: Option<&str>) -> DependentNode {
    let mut node = DependentNode::default();
    node.name = name.to_string();
    node.version = version.to_string();
    node.requires = requires.map(str::to_string);
    node
}

#[test]
fn groups_direct_dependents_by_declared_range_per_installed_version() {
    let mut project = dependent("the root project", "", Some("^0.28.0"));
    project.dep_field = Some(DepField::DevDependencies);
    let mut tsx = dependent("tsx", "4.23.1", Some("~0.28.0"));
    tsx.dependents = Some(vec![dependent("app", "1.0.0", Some("^4.0.0"))]);
    let trees = [
        tree("0.28.1", Some("a"), vec![tsx, dependent("vite", "6.1.0", Some("^0.28.0"))]),
        tree("0.28.1", Some("b"), vec![project, dependent("tsx", "4.23.1", Some("~0.28.0"))]),
        tree("0.18.0", None, vec![dependent("legacy", "1.0.0", None)]),
    ];

    assert_eq!(
        render_dependents_ranges(&trees),
        "esbuild
  Installed: 0.28.1
  Wanted:
    ^0.28.0:
      vite@6.1.0
      the root project
    ~0.28.0:
      tsx@4.23.1

esbuild
  Installed: 0.18.0
  Wanted:
    <unknown>:
      legacy@1.0.0",
    );
}
