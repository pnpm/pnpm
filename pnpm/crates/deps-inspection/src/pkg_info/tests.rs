//! Ports of the upstream `getPkgInfo` tests
//! (deps/inspection/tree-builder/test/getPkgInfo.test.ts).

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use pnpm_lockfile::Lockfile;
use pretty_assertions::assert_eq;

use super::{EdgeContext, PkgInfoEnv, get_pkg_info};
use crate::graph::GraphEdge;

// Port of upstream's 'getPkgInfo handles missing pkgSnapshot without crashing'
// (deps/inspection/tree-builder/test/getPkgInfo.test.ts). Upstream asserts
// `isMissing: true`; `DependencyNode` has no such field, so this port asserts
// the observable fallbacks instead: `name` falls back to the alias and
// `version` to the raw reference.
#[test]
fn get_pkg_info_handles_missing_pkg_snapshot_without_crashing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n\nimporters:\n  .: {}\n",
    )
    .unwrap();
    let lockfile = Lockfile::load_wanted_from_dir(dir.path()).unwrap().unwrap();

    let env = PkgInfoEnv {
        registry_options_by_url: std::collections::BTreeMap::new(),
        registries: HashMap::from([(
            "default".to_string(),
            "https://registry.npmjs.org/".to_string(),
        )]),
        skipped: HashSet::new(),
        current_lockfile: &lockfile,
        wanted_lockfile: Some(&lockfile),
        dep_types: HashMap::new(),
        layout: crate::pkg_info::InspectionLayout {
            lockfile_dir: PathBuf::new(),
            modules_dir: PathBuf::new(),
            relative_modules_dir: PathBuf::from("node_modules"),
            virtual_store_dir: PathBuf::from(".pnpm"),
            virtual_store_dir_max_length: 120,
            store_dir: None,
            is_hoisted: false,
            hoisted_dirs: std::collections::BTreeMap::new(),
        },
    };
    let edge = GraphEdge {
        alias: "missing-pkg".to_string(),
        ref_display: "missing-pkg@1.0.0".to_string(),
        dep_path: Some("missing-pkg@1.0.0".parse().unwrap()),
        link_target: None,
        target: None,
        importer_dependency: None,
    };
    let ctx = EdgeContext {
        peers: None,
        linked_path_base_dir: PathBuf::new(),
        rewrite_link_version_dir: None,
        parent_dir: None,
    };

    let (node, _manifest) = get_pkg_info(&env, &edge, &ctx);

    dbg!(&node);
    assert_eq!(node.alias, "missing-pkg");
    assert_eq!(node.package.name, "missing-pkg");
    assert_eq!(node.package.version, "missing-pkg@1.0.0");
    assert_eq!(
        node.package.path,
        Path::new(".pnpm")
            .join("missing-pkg@1.0.0")
            .join("node_modules")
            .join("missing-pkg")
            .to_string_lossy()
            .into_owned(),
    );
    assert_eq!(node.package.resolved, None);
    assert_eq!(node.status.dev, None);
    assert_eq!(node.package.peers_suffix_hash, None);
    assert!(!node.status.is_peer);
    assert!(!node.status.is_skipped);
    assert!(!node.status.optional);
}

// A lockfile-derived name with traversal segments is never joined into
// the package path (the same guard `pnpm licenses` applies to
// store paths built from lockfile keys).
#[test]
fn resolve_package_path_rejects_traversal_in_lockfile_derived_names() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n\nimporters:\n  .: {}\n",
    )
    .unwrap();
    let lockfile = Lockfile::load_wanted_from_dir(dir.path()).unwrap().unwrap();

    let virtual_store_dir = dir
        .path()
        .join("node_modules")
        .join(".pnpm");
    let env = PkgInfoEnv {
        registry_options_by_url: std::collections::BTreeMap::new(),
        registries: HashMap::from([(
            "default".to_string(),
            "https://registry.npmjs.org/".to_string(),
        )]),
        skipped: HashSet::new(),
        current_lockfile: &lockfile,
        wanted_lockfile: Some(&lockfile),
        dep_types: HashMap::new(),
        layout: crate::pkg_info::InspectionLayout {
            lockfile_dir: dir.path().to_path_buf(),
            modules_dir: dir.path().join("node_modules"),
            relative_modules_dir: PathBuf::from("node_modules"),
            virtual_store_dir: virtual_store_dir.clone(),
            virtual_store_dir_max_length: 120,
            store_dir: None,
            is_hoisted: false,
            hoisted_dirs: std::collections::BTreeMap::new(),
        },
    };
    let ctx = EdgeContext {
        peers: None,
        linked_path_base_dir: PathBuf::new(),
        rewrite_link_version_dir: None,
        parent_dir: None,
    };

    let dep_path = "..@1.0.0".parse().unwrap();
    let path = super::resolve_package_path(
        &env.layout,
        &dep_path,
        "../../../../escape",
        "1.0.0",
        "alias",
        &ctx,
    );

    assert_eq!(path, virtual_store_dir);

    let mut hoisted_layout = env.layout;
    hoisted_layout.is_hoisted = true;
    let hoisted_path = super::resolve_package_path(
        &hoisted_layout,
        &dep_path,
        "../../../../escape",
        "1.0.0",
        "alias",
        &ctx,
    );
    assert_eq!(hoisted_path, virtual_store_dir);
}

#[test]
fn unsafe_path_components_are_detected_by_shape() {
    assert!(super::is_unsafe_path_component(".."));
    assert!(super::is_unsafe_path_component("../../escape"));
    assert!(super::is_unsafe_path_component("/etc"));
    assert!(!super::is_unsafe_path_component("lodash"));
    assert!(!super::is_unsafe_path_component("foo..bar"));
    assert!(!super::is_unsafe_path_component("@scope/pkg"));
    // Rooted-but-prefixless and prefix-only components replace the
    // join base on Windows without being `is_absolute()`.
    #[cfg(windows)]
    {
        assert!(super::is_unsafe_path_component(r"\escape"));
        assert!(super::is_unsafe_path_component("C:evil"));
    }
}

#[test]
fn resolve_package_path_uses_hoisted_dirs_when_linker_is_hoisted() {
    let dir = tempfile::tempdir().unwrap();
    let hoisted_pkg_dir = dir
        .path()
        .join("node_modules")
        .join("foo");
    std::fs::create_dir_all(&hoisted_pkg_dir).unwrap();

    let layout = crate::pkg_info::InspectionLayout {
        lockfile_dir: dir.path().to_path_buf(),
        modules_dir: dir.path().join("node_modules"),
        relative_modules_dir: PathBuf::from("node_modules"),
        virtual_store_dir: dir.path().join("node_modules/.pnpm"),
        virtual_store_dir_max_length: 120,
        store_dir: None,
        is_hoisted: true,
        hoisted_dirs: std::collections::BTreeMap::from([(
            "foo@1.0.0".to_string(),
            vec![hoisted_pkg_dir.clone()],
        )]),
    };
    let ctx = EdgeContext {
        peers: None,
        linked_path_base_dir: dir.path().to_path_buf(),
        rewrite_link_version_dir: None,
        parent_dir: None,
    };
    let dep_path = "foo@1.0.0".parse().unwrap();
    let path = super::resolve_package_path(&layout, &dep_path, "foo", "1.0.0", "foo", &ctx);
    assert_eq!(path, hoisted_pkg_dir);
}

#[test]
fn resolve_package_path_probes_the_alias_when_hoisted_locations_are_missing() {
    let dir = tempfile::tempdir().unwrap();
    let aliased_pkg_dir = dir
        .path()
        .join("node_modules")
        .join("bar");
    std::fs::create_dir_all(&aliased_pkg_dir).unwrap();
    std::fs::write(aliased_pkg_dir.join("package.json"), r#"{"name":"foo","version":"1.0.0"}"#)
        .unwrap();

    let layout = crate::pkg_info::InspectionLayout {
        lockfile_dir: dir.path().to_path_buf(),
        modules_dir: dir.path().join("node_modules"),
        relative_modules_dir: PathBuf::from("node_modules"),
        virtual_store_dir: dir.path().join("node_modules/.pnpm"),
        virtual_store_dir_max_length: 120,
        store_dir: None,
        is_hoisted: true,
        hoisted_dirs: std::collections::BTreeMap::new(),
    };
    let ctx = EdgeContext {
        peers: None,
        linked_path_base_dir: dir.path().to_path_buf(),
        rewrite_link_version_dir: None,
        parent_dir: None,
    };
    let dep_path = "foo@1.0.0".parse().unwrap();
    let path = super::resolve_package_path(&layout, &dep_path, "foo", "1.0.0", "bar", &ctx);
    assert_eq!(path, aliased_pkg_dir);
}

#[test]
fn resolve_package_path_keeps_every_segment_of_a_nested_modules_dir_when_hoisted() {
    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("packages/a");
    let pkg_dir = project_dir.join("www/modules/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(pkg_dir.join("package.json"), r#"{"name":"foo","version":"1.0.0"}"#).unwrap();

    let layout = crate::pkg_info::InspectionLayout {
        lockfile_dir: dir.path().to_path_buf(),
        modules_dir: dir.path().join("www/modules"),
        relative_modules_dir: PathBuf::from("www/modules"),
        virtual_store_dir: dir.path().join("www/modules/.pnpm"),
        virtual_store_dir_max_length: 120,
        store_dir: None,
        is_hoisted: true,
        hoisted_dirs: std::collections::BTreeMap::new(),
    };
    let ctx = EdgeContext {
        peers: None,
        linked_path_base_dir: project_dir,
        rewrite_link_version_dir: None,
        parent_dir: None,
    };
    let dep_path = "foo@1.0.0".parse().unwrap();
    let path = super::resolve_package_path(&layout, &dep_path, "foo", "1.0.0", "foo", &ctx);
    assert_eq!(path, pkg_dir);
}

#[test]
fn resolve_package_path_picks_the_hoisted_copy_node_resolves_from_the_parent() {
    let dir = tempfile::tempdir().unwrap();
    let root_copy = dir.path().join("node_modules/foo");
    let project_copy = dir.path().join("packages/a/node_modules/foo");
    let parent_dir = dir.path().join("packages/a/node_modules/bar");
    for created in [&root_copy, &project_copy, &parent_dir] {
        std::fs::create_dir_all(created).unwrap();
    }

    let layout = crate::pkg_info::InspectionLayout {
        lockfile_dir: dir.path().to_path_buf(),
        modules_dir: dir.path().join("node_modules"),
        relative_modules_dir: PathBuf::from("node_modules"),
        virtual_store_dir: dir.path().join("node_modules/.pnpm"),
        virtual_store_dir_max_length: 120,
        store_dir: None,
        is_hoisted: true,
        hoisted_dirs: std::collections::BTreeMap::from([(
            "foo@1.0.0".to_string(),
            vec![root_copy, project_copy.clone()],
        )]),
    };
    let ctx = EdgeContext {
        peers: None,
        linked_path_base_dir: dir.path().to_path_buf(),
        rewrite_link_version_dir: None,
        parent_dir: Some(parent_dir),
    };
    let dep_path = "foo@1.0.0".parse().unwrap();
    let path = super::resolve_package_path(&layout, &dep_path, "foo", "1.0.0", "foo", &ctx);
    assert_eq!(path, project_copy);
}

#[test]
fn resolve_package_path_picks_the_hoisted_copy_under_the_edge_alias() {
    let dir = tempfile::tempdir().unwrap();
    let foo_copy = dir.path().join("node_modules/foo");
    let bar_copy = dir.path().join("node_modules/bar");
    let scoped_copy = dir.path().join("node_modules/@scope/foo");
    for created in [&foo_copy, &bar_copy, &scoped_copy] {
        std::fs::create_dir_all(created).unwrap();
    }

    let layout = crate::pkg_info::InspectionLayout {
        lockfile_dir: dir.path().to_path_buf(),
        modules_dir: dir.path().join("node_modules"),
        relative_modules_dir: PathBuf::from("node_modules"),
        virtual_store_dir: dir.path().join("node_modules/.pnpm"),
        virtual_store_dir_max_length: 120,
        store_dir: None,
        is_hoisted: true,
        hoisted_dirs: std::collections::BTreeMap::from([(
            "foo@1.0.0".to_string(),
            vec![scoped_copy.clone(), foo_copy.clone(), bar_copy.clone()],
        )]),
    };
    let ctx = EdgeContext {
        peers: None,
        linked_path_base_dir: dir.path().to_path_buf(),
        rewrite_link_version_dir: None,
        parent_dir: None,
    };
    let dep_path = "foo@1.0.0".parse().unwrap();
    let resolve =
        |alias| super::resolve_package_path(&layout, &dep_path, "foo", "1.0.0", alias, &ctx);
    assert_eq!(resolve("foo"), foo_copy);
    assert_eq!(resolve("bar"), bar_copy);
    assert_eq!(resolve("@scope/foo"), scoped_copy);

    std::fs::remove_dir(&foo_copy).unwrap();
    assert_ne!(resolve("foo"), foo_copy, "a copy on disk wins over a missing one under the alias");
}

#[test]
fn get_pkg_info_preserves_non_registry_refs_and_extracts_registry_versions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pnpm-lock.yaml"),
        "\
lockfileVersion: '9.0'

importers:
  .: {}

packages:
  my-local-pkg@file:local-pkg:
    resolution: {type: directory, directory: local-pkg}

  '@example/local@file:@scoped-dir/local':
    resolution: {type: directory, directory: '@scoped-dir/local'}

  tgz-pkg@file:pkg.tgz:
    resolution: {integrity: sha512-tarball, tarball: file:pkg.tgz}
    version: 1.2.3

  node@runtime:26.13.0:
    resolution: {type: variations, variants: []}

  foo@work:1.0.0:
    resolution: {integrity: sha512-foo}

snapshots:
  my-local-pkg@file:local-pkg: {}
  '@example/local@file:@scoped-dir/local': {}
  tgz-pkg@file:pkg.tgz: {}
  node@runtime:26.13.0: {}
  foo@work:1.0.0: {}
",
    )
    .unwrap();
    let lockfile = Lockfile::load_wanted_from_dir(dir.path()).unwrap().unwrap();
    let env = PkgInfoEnv {
        registry_options_by_url: std::collections::BTreeMap::new(),
        registries: HashMap::from([(
            "default".to_string(),
            "https://registry.npmjs.org/".to_string(),
        )]),
        skipped: HashSet::new(),
        current_lockfile: &lockfile,
        wanted_lockfile: Some(&lockfile),
        dep_types: HashMap::new(),
        layout: crate::pkg_info::InspectionLayout {
            lockfile_dir: dir.path().to_path_buf(),
            modules_dir: dir.path().join("node_modules"),
            relative_modules_dir: PathBuf::from("node_modules"),
            virtual_store_dir: dir.path().join("node_modules/.pnpm"),
            virtual_store_dir_max_length: 120,
            store_dir: None,
            is_hoisted: false,
            hoisted_dirs: std::collections::BTreeMap::new(),
        },
    };
    let ctx = EdgeContext {
        peers: None,
        linked_path_base_dir: dir.path().to_path_buf(),
        rewrite_link_version_dir: None,
        parent_dir: None,
    };
    let resolve_edge = |alias: &str, ref_display: &str, dep_path: &str| {
        let edge = GraphEdge {
            alias: alias.to_string(),
            ref_display: ref_display.to_string(),
            dep_path: Some(dep_path.parse().unwrap()),
            link_target: None,
            target: None,
            importer_dependency: None,
        };
        get_pkg_info(&env, &edge, &ctx).0
    };

    let aliased_file =
        resolve_edge("my-alias", "my-local-pkg@file:local-pkg", "my-local-pkg@file:local-pkg");
    assert_eq!(aliased_file.alias, "my-alias");
    assert_eq!(aliased_file.package.name, "my-local-pkg");
    assert_eq!(aliased_file.package.version, "my-local-pkg@file:local-pkg");

    let unaliased_file =
        resolve_edge("my-local-pkg", "file:local-pkg", "my-local-pkg@file:local-pkg");
    assert_eq!(unaliased_file.alias, "my-local-pkg");
    assert_eq!(unaliased_file.package.name, "my-local-pkg");
    assert_eq!(unaliased_file.package.version, "file:local-pkg");

    let scoped_aliased_file = resolve_edge(
        "scoped-alias",
        "@example/local@file:@scoped-dir/local",
        "@example/local@file:@scoped-dir/local",
    );
    assert_eq!(scoped_aliased_file.package.name, "@example/local");
    assert_eq!(scoped_aliased_file.package.version, "@example/local@file:@scoped-dir/local");

    let tarball_alias = resolve_edge("tgz-alias", "tgz-pkg@file:pkg.tgz", "tgz-pkg@file:pkg.tgz");
    assert_eq!(tarball_alias.package.name, "tgz-pkg");
    assert_eq!(tarball_alias.package.version, "1.2.3");

    let runtime_dep = resolve_edge("node", "runtime:26.13.0", "node@runtime:26.13.0");
    assert_eq!(runtime_dep.package.name, "node");
    assert_eq!(runtime_dep.package.version, "26.13.0");
    assert_eq!(runtime_dep.package.resolved, None);

    let registry_qualified = resolve_edge("foo", "work:1.0.0", "foo@work:1.0.0");
    assert_eq!(registry_qualified.package.name, "foo");
    assert_eq!(registry_qualified.package.version, "1.0.0");
    assert_eq!(
        registry_qualified.package.resolved.as_deref(),
        Some("https://registry.npmjs.org/foo/-/foo-1.0.0.tgz"),
    );

    use crate::dependents::name_ver_from_dep_path;
    assert_eq!(
        name_ver_from_dep_path(&lockfile, &"my-local-pkg@file:local-pkg".parse().unwrap()),
        ("my-local-pkg".to_string(), String::new()),
    );
    assert_eq!(
        name_ver_from_dep_path(&lockfile, &"tgz-pkg@file:pkg.tgz".parse().unwrap()),
        ("tgz-pkg".to_string(), "1.2.3".to_string()),
    );
    let (runtime_name, runtime_ver) =
        name_ver_from_dep_path(&lockfile, &"node@runtime:26.13.0".parse().unwrap());
    assert_eq!((runtime_name.as_str(), runtime_ver.as_str()), ("node", "26.13.0"));
    assert!(
        crate::search::Searcher::from_queries(&["node@26".to_string()])
            .unwrap()
            .matches(&runtime_name, &runtime_name, &runtime_ver, None)
            .is_match(),
    );
    assert_eq!(
        name_ver_from_dep_path(&lockfile, &"foo@work:1.0.0".parse().unwrap()),
        ("foo".to_string(), "1.0.0".to_string()),
    );
}
