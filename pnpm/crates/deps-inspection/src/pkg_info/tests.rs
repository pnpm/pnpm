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
            modules_dir_name: PathBuf::from("node_modules"),
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
            modules_dir_name: PathBuf::from("node_modules"),
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
        modules_dir_name: PathBuf::from("node_modules"),
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
