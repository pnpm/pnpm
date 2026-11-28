use std::path::Path;

use pnpm_lockfile::Lockfile;

use super::package_merge_id;

#[test]
fn package_merge_id_resolves_directories_lexically_and_preserves_unc_paths() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pnpm-lock.yaml"),
        r"lockfileVersion: '9.0'

importers:
  .: {}

packages:
  foo@file:../../vendor/foo-v1:
    resolution: {type: directory, directory: ../../vendor/foo-v1}

  foo@file:../../vendor/foo-v1/:
    resolution: {type: directory, directory: ../../vendor/foo-v1/}

  foo@file:../../../vendor/foo-v1:
    resolution: {type: directory, directory: ../../../vendor/foo-v1}

  foo@file:../../vendor/foo-v2:
    resolution: {type: directory, directory: ../../vendor/foo-v2}

  unc-bs@file:share:
    resolution: {type: directory, directory: '\\server\share\foo'}

  unc-slash@file:share:
    resolution: {type: directory, directory: '//server/share/foo/'}

snapshots:
  foo@file:../../vendor/foo-v1: {}
  foo@file:../../vendor/foo-v1/: {}
  foo@file:../../../vendor/foo-v1: {}
  foo@file:../../vendor/foo-v2: {}
  unc-bs@file:share: {}
  unc-slash@file:share: {}
",
    )
    .unwrap();
    let lockfile = Lockfile::load_wanted_from_dir(dir.path()).unwrap().unwrap();

    let p1 = package_merge_id(
        Path::new("/workspace/packages/project-1"),
        &lockfile,
        &"foo@file:../../vendor/foo-v1".parse().unwrap(),
        "",
    );
    let p1_trailing = package_merge_id(
        Path::new("/workspace/packages/project-1"),
        &lockfile,
        &"foo@file:../../vendor/foo-v1/".parse().unwrap(),
        "",
    );
    let p2 = package_merge_id(
        Path::new("/workspace/packages/nested/project-2"),
        &lockfile,
        &"foo@file:../../../vendor/foo-v1".parse().unwrap(),
        "",
    );
    let p3 = package_merge_id(
        Path::new("/workspace/packages/project-3"),
        &lockfile,
        &"foo@file:../../vendor/foo-v2".parse().unwrap(),
        "",
    );
    assert_eq!(p1, p1_trailing);
    assert_eq!(p1, p2);
    assert_ne!(p1, p3);

    let unc_bs = package_merge_id(
        Path::new("/workspace/packages/project-1"),
        &lockfile,
        &"unc-bs@file:share".parse().unwrap(),
        "",
    );
    let unc_slash = package_merge_id(
        Path::new("/workspace/packages/project-1"),
        &lockfile,
        &"unc-slash@file:share".parse().unwrap(),
        "",
    );
    assert_eq!(unc_bs.as_deref(), Some("dir://server/share/foo"));
    assert_eq!(unc_bs, unc_slash);
}

const LENGTH: usize = 1_000;

/// A lockfile whose importer depends on `p0`, which depends on `p1`, and so
/// on to `p999`.
fn chain_lockfile(dir: &Path) -> Lockfile {
    use std::fmt::Write;
    let mut packages = String::new();
    let mut snapshots = String::new();
    for index in 0..LENGTH {
        writeln!(packages, "  p{index}@1.0.0:\n    resolution: {{integrity: sha512-p{index}}}")
            .unwrap();
        if index + 1 < LENGTH {
            let child = index + 1;
            writeln!(snapshots, "  p{index}@1.0.0:\n    dependencies:\n      p{child}: 1.0.0")
                .unwrap();
        } else {
            writeln!(snapshots, "  p{index}@1.0.0: {{}}").unwrap();
        }
    }
    let importers = "importers:\n  .:\n    dependencies:\n      p0:\n        specifier: 1.0.0\n        \
                     version: 1.0.0\n";
    std::fs::write(
        dir.join("pnpm-lock.yaml"),
        format!("lockfileVersion: '9.0'\n{importers}packages:\n{packages}snapshots:\n{snapshots}"),
    )
    .unwrap();
    Lockfile::load_wanted_from_dir(dir).unwrap().unwrap()
}

fn env_for<'lockfile>(
    dir: &Path,
    lockfile: &'lockfile Lockfile,
) -> crate::pkg_info::PkgInfoEnv<'lockfile> {
    use std::collections::{
        BTreeMap,
        HashMap,
        HashSet,
    };
    crate::pkg_info::PkgInfoEnv {
        registry_options_by_url: BTreeMap::new(),
        registries: HashMap::from([(
            "default".to_string(),
            "https://registry.npmjs.org/".to_string(),
        )]),
        skipped: HashSet::new(),
        current_lockfile: lockfile,
        wanted_lockfile: Some(lockfile),
        dep_types: HashMap::new(),
        layout: crate::pkg_info::InspectionLayout {
            lockfile_dir: dir.to_path_buf(),
            modules_dir: dir.join("node_modules"),
            relative_modules_dir: std::path::PathBuf::from("node_modules"),
            virtual_store_dir: dir.join("node_modules/.pnpm"),
            virtual_store_dir_max_length: 120,
            store_dir: None,
            is_hoisted: false,
            hoisted_dirs: BTreeMap::new(),
        },
    }
}

/// An importer whose dependency `p0` heads a chain a thousand snapshots
/// deep resolves a location for every package of the chain.
#[test]
fn every_package_of_a_deep_chain_is_resolved() {
    use crate::graph::{
        BoundPeerEdges,
        BuildGraphOptions,
        build_dependency_graph,
    };
    let dir = tempfile::tempdir().unwrap();
    let lockfile = chain_lockfile(dir.path());
    let graph = build_dependency_graph(
        vec![crate::TreeNodeId::Importer(".".to_string())],
        &BuildGraphOptions {
            lockfile: &lockfile,
            include: pnpm_modules_yaml::IncludedDependencies {
                dependencies: true,
                dev_dependencies: true,
                optional_dependencies: true,
            },
            only_projects: false,
            peer_edges: pnpm_lockfile::PeerEdgeOptions::default(),
            bound_peer_edges: BoundPeerEdges::Follow,
        },
    );
    let resolved = super::resolve_package_nodes(&env_for(dir.path(), &lockfile), &graph);
    assert_eq!(resolved.len(), LENGTH);
}
