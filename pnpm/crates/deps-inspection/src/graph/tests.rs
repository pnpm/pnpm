use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::IncludedDependencies;

use super::{BuildGraphOptions, build_dependency_graph};
use crate::TreeNodeId;

fn lockfile() -> Lockfile {
    let contents = r"lockfileVersion: '9.0'
packages:
  'app@1.0.0':
    resolution: {integrity: 'sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg=='}
  'yarn@tool:1.22.22':
    resolution: {integrity: 'sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg=='}
snapshots:
  'app@1.0.0':
    buildDependencies:
      yarn: 'yarn@tool:1.22.22'
  'yarn@tool:1.22.22': {}
";
    serde_saphyr::from_str(contents).unwrap()
}

#[test]
fn build_dependencies_are_hidden_by_default_and_included_on_request() {
    let lockfile = lockfile();
    let root = [TreeNodeId::Package("app@1.0.0".parse().unwrap())];
    let build = |include_build_dependencies| {
        build_dependency_graph(
            &root,
            &BuildGraphOptions {
                lockfile: &lockfile,
                include: IncludedDependencies {
                    dependencies: true,
                    dev_dependencies: true,
                    optional_dependencies: true,
                },
                only_projects: false,
                include_build_dependencies,
                peer_edges: pnpm_lockfile::PeerEdgeOptions::default(),
            },
        )
    };
    let package = root[0].clone();
    let without_build_dependencies = build(false);
    assert!(without_build_dependencies.nodes[&package].edges.is_empty());
    let with_build_dependencies = build(true);
    assert_eq!(with_build_dependencies.nodes[&package].edges[0].alias, "yarn");
    assert_eq!(
        with_build_dependencies.nodes[&package].edges[0].target,
        Some(TreeNodeId::Package("yarn@tool:1.22.22".parse().unwrap()))
    );
}
