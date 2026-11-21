use std::{
    collections::{
        BTreeMap,
        HashMap,
        HashSet,
    },
    fs,
    path::PathBuf,
};

use pnpm_lockfile::{
    Lockfile,
    PeerEdgeOptions,
};
use pnpm_modules_yaml::IncludedDependencies;
use pretty_assertions::assert_eq;

use super::{
    BuildDependentsOptions,
    DependencyKind,
    DependencySpecifiers,
    ManifestProjector,
    build_dependents_tree,
};
use crate::{
    TreeNodeId,
    graph::{
        BoundPeerEdges,
        BuildGraphOptions,
        build_dependency_graph,
    },
    pkg_info::{
        InspectionLayout,
        ManifestSource,
        PkgInfoEnv,
    },
    search::Searcher,
};

#[test]
fn specifiers_and_projection_share_a_query_scoped_manifest_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("package.json");
    fs::write(
        &path,
        r#"{"license":"MIT","dependencies":{"target":"^1.0.0"},"peerDependencies":{"target":"~1.0.0"}}"#,
    )
    .unwrap();
    let node_id = TreeNodeId::Package("parent@1.0.0".parse().unwrap());
    let resolved = HashMap::from([(
        node_id.clone(),
        ManifestSource {
            path: dir.path().to_path_buf(),
            integrity: None,
            name: "parent".to_string(),
            version: "1.0.0".to_string(),
        },
    )]);
    let fields = ["license".to_string()];
    let reader = ManifestProjector::new(&fields, &resolved);
    assert_eq!(
        reader.dependency_specifier(&node_id, "target", DependencyKind::Dependency).as_deref(),
        Some("^1.0.0"),
    );
    fs::remove_file(&path).unwrap();
    assert_eq!(reader.project(&node_id).unwrap()["license"], "MIT");
    assert_eq!(
        reader.dependency_specifier(&node_id, "target", DependencyKind::Peer).as_deref(),
        Some("~1.0.0"),
    );
    let next_query = ManifestProjector::new(&fields, &resolved);
    assert_eq!(
        next_query.dependency_specifier(&node_id, "target", DependencyKind::Dependency),
        None,
    );
}

fn write_workspace(dir: &std::path::Path) -> Lockfile {
    fs::write(
        dir.join("pnpm-lock.yaml"),
        r"lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      dep:
        specifier: ~1.0.0
        version: 1.0.0

packages:

  dep@1.0.0:
    resolution: {integrity: sha512-dep}

  nested@2.0.0:
    resolution: {integrity: sha512-nested}

snapshots:

  dep@1.0.0:
    dependencies:
      nested: 2.0.0

  nested@2.0.0: {}
",
    )
    .unwrap();
    let dep_dir = dir.join("node_modules/.pnpm/dep@1.0.0/node_modules/dep");
    fs::create_dir_all(&dep_dir).unwrap();
    fs::write(
        dep_dir.join("package.json"),
        r#"{"name":"dep","version":"1.0.0","dependencies":{"nested":"^2.0.0"}}"#,
    )
    .unwrap();
    Lockfile::load_wanted_from_dir(dir).unwrap().unwrap()
}

fn requires_of_dependents(
    dir: &std::path::Path,
    lockfile: &Lockfile,
    dependency_specifiers: DependencySpecifiers,
) -> (Option<String>, Option<String>) {
    let env = PkgInfoEnv {
        registry_options_by_url: BTreeMap::new(),
        registries: HashMap::from([(
            "default".to_string(),
            "https://registry.npmjs.org/".to_string(),
        )]),
        skipped: HashSet::new(),
        current_lockfile: lockfile,
        wanted_lockfile: Some(lockfile),
        dep_types: HashMap::new(),
        layout: InspectionLayout {
            lockfile_dir: dir.to_path_buf(),
            modules_dir: dir.join("node_modules"),
            relative_modules_dir: PathBuf::from("node_modules"),
            virtual_store_dir: dir.join("node_modules/.pnpm"),
            virtual_store_dir_max_length: 120,
            store_dir: None,
            is_hoisted: false,
            hoisted_dirs: BTreeMap::new(),
        },
    };
    let graph = build_dependency_graph(
        vec![TreeNodeId::Importer(".".to_string())],
        &BuildGraphOptions {
            lockfile,
            include: IncludedDependencies {
                dependencies: true,
                dev_dependencies: true,
                optional_dependencies: true,
            },
            only_projects: false,
            peer_edges: PeerEdgeOptions::default(),
            bound_peer_edges: BoundPeerEdges::Follow,
        },
    );
    let trees = build_dependents_tree(&BuildDependentsOptions {
        env: &env,
        graph: &graph,
        search: &Searcher::from_queries(&["nested".to_string()]).unwrap(),
        importer_info: &HashMap::new(),
        manifest_fields: &[],
        dependency_specifiers,
    });
    let dep = &trees[0].dependents[0];
    let importer = &dep.dependents.as_ref().unwrap()[0];
    dbg!(&trees);
    (dep.requires.clone(), importer.requires.clone())
}

#[test]
fn dependency_specifiers_fill_the_requested_levels() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = write_workspace(dir.path());

    for (specifiers, expected) in [
        (DependencySpecifiers::All, (Some("^2.0.0"), Some("~1.0.0"))),
        (DependencySpecifiers::DirectDependents, (Some("^2.0.0"), None)),
        (DependencySpecifiers::Omit, (None, None)),
    ] {
        let (dep, importer) = requires_of_dependents(dir.path(), &lockfile, specifiers);
        assert_eq!((dep.as_deref(), importer.as_deref()), expected, "{specifiers:?}");
    }
}
