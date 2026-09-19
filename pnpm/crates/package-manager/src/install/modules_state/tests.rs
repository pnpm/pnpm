use super::frozen_tree_intact;
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{Lockfile, ProjectSnapshot, SnapshotEntry};
use pnpm_modules_yaml::{
    Host, IncludedDependencies, Modules, ModulesLayout, write_modules_manifest,
};
use std::{fs, path::Path};
use tempfile::tempdir;
use text_block_macros::text_block;

mod peer_variants;

fn shared_workspace_tree_intact(root: &Path, node_linker: NodeLinker) -> bool {
    tree_intact(root, node_linker, &shared_workspace_lockfile())
}

fn shared_workspace_lockfile() -> Lockfile {
    serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  packages/a:"
        "    dependencies:"
        "      foo:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "  packages/b:"
        "    dependencies:"
        "      foo:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "snapshots:"
        "  foo@1.0.0: {}"
    })
    .expect("parse the two sibling importers")
}

fn tree_intact(root: &Path, node_linker: NodeLinker, lockfile: &Lockfile) -> bool {
    let mut config = Config::new();
    config.modules_dir = root.join("node_modules");
    config.virtual_store_dir = config.modules_dir.join(".pnpm");
    let modules = ModulesLayout {
        included: IncludedDependencies {
            dependencies: true,
            dev_dependencies: false,
            optional_dependencies: false,
        },
        ..ModulesLayout::default()
    };
    frozen_tree_intact(lockfile, &modules, &config, root, node_linker)
}

fn record_hoisted_locations(root: &Path, locations: &[(&str, &[&str])]) {
    let modules = Modules {
        hoisted_locations: Some(
            locations
                .iter()
                .map(|(key, dirs)| {
                    (
                        key.to_string(),
                        dirs.iter()
                            .map(ToString::to_string)
                            .collect(),
                    )
                })
                .collect(),
        ),
        ..Modules::default()
    };
    write_modules_manifest::<Host>(&root.join("node_modules"), modules).unwrap();
}

#[test]
fn hoisted_importers_resolve_a_shared_dependency_at_the_workspace_root() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("workspace");
    fs::create_dir_all(root.join("node_modules/foo")).unwrap();
    record_hoisted_locations(&root, &[("foo@1.0.0", &["node_modules/foo"])]);

    assert!(shared_workspace_tree_intact(&root, NodeLinker::Hoisted));

    fs::remove_dir(root.join("node_modules/foo")).unwrap();
    assert!(!shared_workspace_tree_intact(&root, NodeLinker::Hoisted));
}

#[test]
fn hoisted_importers_do_not_resolve_a_missing_dependency_outside_the_workspace() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("workspace");
    fs::create_dir_all(root.join("node_modules")).unwrap();
    fs::create_dir_all(dir.path().join("node_modules/foo")).unwrap();
    record_hoisted_locations(&root, &[("foo@1.0.0", &["../node_modules/foo"])]);

    assert!(!shared_workspace_tree_intact(&root, NodeLinker::Hoisted));
}

#[test]
fn hoisted_importers_do_not_resolve_a_dependency_from_a_sibling() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("workspace");
    fs::create_dir_all(root.join("packages/a/node_modules/foo")).unwrap();
    record_hoisted_locations(&root, &[("foo@1.0.0", &["packages/a/node_modules/foo"])]);

    assert!(!shared_workspace_tree_intact(&root, NodeLinker::Hoisted));
}

#[test]
fn isolated_importers_still_require_their_own_dependency_links() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("workspace");
    fs::create_dir_all(root.join("node_modules/foo")).unwrap();
    fs::create_dir_all(root.join("node_modules/.pnpm/foo@1.0.0/node_modules/foo")).unwrap();

    assert!(!shared_workspace_tree_intact(&root, NodeLinker::Isolated));

    for project in ["a", "b"] {
        fs::create_dir_all(
            root.join("packages")
                .join(project)
                .join("node_modules/foo"),
        )
        .unwrap();
    }
    assert!(shared_workspace_tree_intact(&root, NodeLinker::Isolated));
}

#[test]
fn a_missing_nested_hoisted_version_cannot_fall_back_to_another_version() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("workspace");
    let nested = root.join("packages/b/node_modules/foo");
    fs::create_dir_all(root.join("node_modules/foo")).unwrap();
    fs::create_dir_all(&nested).unwrap();
    record_hoisted_locations(
        &root,
        &[("foo@1.0.0", &["node_modules/foo"]), ("foo@2.0.0", &["packages/b/node_modules/foo"])],
    );
    let mut lockfile = shared_workspace_lockfile();
    lockfile.importers
        .get_mut("packages/b")
        .unwrap()
        .dependencies
        .as_mut()
        .unwrap()
        .get_mut(&"foo".parse().unwrap())
        .unwrap()
        .version = serde_saphyr::from_str("2.0.0").unwrap();
    lockfile.snapshots
        .as_mut()
        .unwrap()
        .insert("foo@2.0.0".parse().unwrap(), SnapshotEntry::default());
    assert!(tree_intact(&root, NodeLinker::Hoisted, &lockfile));

    fs::remove_dir(nested).unwrap();
    assert!(!tree_intact(&root, NodeLinker::Hoisted, &lockfile));
    assert!(
        shared_workspace_tree_intact(&root, NodeLinker::Hoisted),
        "a placement outside the current graph does not require materialization",
    );
}

#[test]
fn hoisted_dependencies_require_recorded_placements() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("workspace");
    fs::create_dir_all(root.join("node_modules/foo")).unwrap();
    assert!(!shared_workspace_tree_intact(&root, NodeLinker::Hoisted));

    record_hoisted_locations(&root, &[]);
    assert!(!shared_workspace_tree_intact(&root, NodeLinker::Hoisted));
    record_hoisted_locations(&root, &[("foo@1.0.0", &[])]);
    assert!(!shared_workspace_tree_intact(&root, NodeLinker::Hoisted));
    record_hoisted_locations(&root, &[("not-a-dep-path", &["node_modules/foo"])]);
    assert!(!shared_workspace_tree_intact(&root, NodeLinker::Hoisted));
}

#[test]
fn skipped_hoisted_packages_do_not_require_placements() {
    let dir = tempdir().unwrap();
    let mut config = Config::new();
    config.modules_dir = dir.path().join("node_modules");
    let modules = ModulesLayout {
        included: IncludedDependencies {
            dependencies: true,
            dev_dependencies: false,
            optional_dependencies: false,
        },
        skipped: vec!["foo@1.0.0".to_string()],
        ..ModulesLayout::default()
    };
    assert!(frozen_tree_intact(
        &shared_workspace_lockfile(),
        &modules,
        &config,
        dir.path(),
        NodeLinker::Hoisted
    ));
}

#[test]
fn hoisted_placements_cannot_resolve_to_an_external_package() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("workspace");
    let outside = dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    record_hoisted_locations(&root, &[("foo@1.0.0", &["node_modules/foo"])]);
    pnpm_fs::symlink_dir(&outside, &root.join("node_modules/foo")).unwrap();
    assert!(!shared_workspace_tree_intact(&root, NodeLinker::Hoisted));
}

#[test]
fn malformed_importer_paths_cannot_short_circuit_materialization() {
    let dir = tempdir().unwrap();
    for importer in ["../outside", "/absolute", "packages/../../outside"] {
        let mut lockfile = shared_workspace_lockfile();
        lockfile.importers.clear();
        lockfile.importers.insert(importer.to_string(), ProjectSnapshot::default());
        lockfile.snapshots = None;
        assert!(!tree_intact(dir.path(), NodeLinker::Isolated, &lockfile), "{importer}");
    }
}

#[test]
fn malformed_dependency_names_cannot_short_circuit_materialization() {
    let dir = tempdir().unwrap();
    for name in ["../outside", "@scope/../../outside", "/absolute"] {
        let mut lockfile = shared_workspace_lockfile();
        lockfile.importers.retain(|id, _| id == "packages/a");
        let dependencies = lockfile.importers
            .get_mut("packages/a")
            .unwrap()
            .dependencies
            .as_mut()
            .unwrap();
        let dependency = dependencies
            .remove(&"foo".parse().unwrap())
            .unwrap();
        dependencies.insert(name.parse().unwrap(), dependency);
        lockfile.snapshots = None;
        assert!(!tree_intact(dir.path(), NodeLinker::Isolated, &lockfile), "{name}");
    }
}
