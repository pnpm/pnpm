use super::prune_direct_deps_excluded_by_groups;
use pacquet_config::Config;
use pacquet_lockfile::{
    ComVer, Lockfile, LockfileVersion, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec,
};
use pacquet_modules_yaml::IncludedDependencies;
use pacquet_testing_utils::fs::is_symlink_or_junction;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};
use tempfile::tempdir;

fn lockfile_with_root_importer(snapshot: ProjectSnapshot) -> Lockfile {
    let mut importers = HashMap::new();
    importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), snapshot);
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer { major: 9, minor: 0 }).unwrap(),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers,
        packages: None,
        snapshots: None,
    }
}

const FULL: IncludedDependencies = IncludedDependencies {
    dependencies: true,
    dev_dependencies: true,
    optional_dependencies: true,
};
const PROD_ONLY: IncludedDependencies = IncludedDependencies {
    dependencies: true,
    dev_dependencies: false,
    optional_dependencies: false,
};

fn dep_map(entries: &[(&str, &str)]) -> ResolvedDependencyMap {
    let mut map = ResolvedDependencyMap::new();
    for (name, version) in entries {
        map.insert(
            name.parse().expect("parse pkg name"),
            ResolvedDependencySpec {
                specifier: format!("^{version}"),
                version: version.parse::<pacquet_lockfile::PkgVerPeer>().unwrap().into(),
            },
        );
    }
    map
}

/// Lay down a direct-dep symlink the way the isolated linker would:
/// a real package dir in the virtual store (with a `package.json`
/// declaring `bin` when given) and a `node_modules/<name>` symlink
/// pointing at it. The target must exist for Windows junctions.
fn link_dep(modules_dir: &Path, name: &str, bin: Option<&str>) {
    let target =
        modules_dir.join(".pacquet").join(name.replace('/', "+")).join("node_modules").join(name);
    fs::create_dir_all(&target).expect("create symlink target");
    let manifest = match bin {
        Some(bin_name) => {
            fs::write(target.join("cli.js"), b"#!/usr/bin/env node\n").expect("write bin script");
            format!(r#"{{"name":{name:?},"version":"1.0.0","bin":{{{bin_name:?}:"./cli.js"}}}}"#)
        }
        None => format!(r#"{{"name":{name:?},"version":"1.0.0"}}"#),
    };
    fs::write(target.join("package.json"), manifest).expect("write package.json");
    let link = modules_dir.join(name);
    fs::create_dir_all(link.parent().unwrap()).expect("create link parent");
    pacquet_fs::symlink_dir(&target, &link).expect("create direct-dep symlink");
}

/// A full → `--prod` switch must remove exactly the dev dep's symlink
/// and its bin shim: the prod dep's link, the user's own real file and
/// directory (even one shadowing a former dep name), and unrelated
/// shims all survive.
#[test]
fn removes_only_excluded_direct_dep_links_and_their_bins() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let modules_dir = workspace_root.join("node_modules");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    link_dep(&modules_dir, "keep-me", None);
    link_dep(&modules_dir, "@scope/dev-dep", Some("devtool"));
    // Shims as the bin linker would have written them.
    let bins_dir = modules_dir.join(".bin");
    fs::create_dir_all(&bins_dir).unwrap();
    fs::write(bins_dir.join("devtool"), b"#!/bin/sh\n").unwrap();
    fs::write(bins_dir.join("unrelated-tool"), b"#!/bin/sh\n").unwrap();
    // The user's own entries: a stray file, and a real directory whose
    // name collides with a dev dep recorded in the lockfile.
    fs::write(modules_dir.join("vendored.txt"), b"keep me").unwrap();
    fs::create_dir_all(modules_dir.join("user-owned-dir")).unwrap();

    let current_lockfile = lockfile_with_root_importer(ProjectSnapshot {
        dependencies: Some(dep_map(&[("keep-me", "1.0.0")])),
        dev_dependencies: Some(dep_map(&[
            ("@scope/dev-dep", "1.0.0"),
            ("user-owned-dir", "1.0.0"),
        ])),
        ..ProjectSnapshot::default()
    });

    prune_direct_deps_excluded_by_groups(
        &current_lockfile,
        FULL,
        PROD_ONLY,
        workspace_root,
        config,
        None,
    )
    .expect("prune should succeed");

    assert!(modules_dir.join("@scope/dev-dep").symlink_metadata().is_err());
    assert!(!bins_dir.join("devtool").exists());
    assert!(is_symlink_or_junction(&modules_dir.join("keep-me")).unwrap());
    assert!(bins_dir.join("unrelated-tool").exists());
    assert!(modules_dir.join("vendored.txt").exists());
    assert!(modules_dir.join("user-owned-dir").is_dir());
}

/// A `node_modules` that is itself a symlink resolving outside the
/// workspace root fails the containment check, so nothing behind it is
/// touched — the same refusal the purge applies.
#[test]
fn refuses_to_prune_through_a_modules_dir_escaping_the_workspace() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path().join("workspace");
    let outside = dir.path().join("outside-modules");
    fs::create_dir_all(&workspace_root).unwrap();

    // Seed the external dir with a linked dep + shim, then point the
    // workspace's node_modules at it.
    link_dep(&outside, "dev-dep", Some("devtool"));
    let outside_bins = outside.join(".bin");
    fs::create_dir_all(&outside_bins).unwrap();
    fs::write(outside_bins.join("devtool"), b"#!/bin/sh\n").unwrap();
    let modules_dir = workspace_root.join("node_modules");
    pacquet_fs::symlink_dir(&outside, &modules_dir).unwrap();

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    let current_lockfile = lockfile_with_root_importer(ProjectSnapshot {
        dev_dependencies: Some(dep_map(&[("dev-dep", "1.0.0")])),
        ..ProjectSnapshot::default()
    });

    prune_direct_deps_excluded_by_groups(
        &current_lockfile,
        FULL,
        PROD_ONLY,
        &workspace_root,
        config,
        None,
    )
    .expect("prune should succeed");

    assert!(is_symlink_or_junction(&outside.join("dev-dep")).unwrap());
    assert!(outside_bins.join("devtool").exists());
}

/// A `.bin` that is itself a symlink must not have shims deleted
/// through it, while the dep's own link is still pruned.
#[test]
fn skips_shim_removal_through_a_symlinked_bin_dir() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let modules_dir = workspace_root.join("node_modules");
    let outside_bins = workspace_root.join("outside-bins");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    link_dep(&modules_dir, "dev-dep", Some("devtool"));
    fs::create_dir_all(&outside_bins).unwrap();
    fs::write(outside_bins.join("devtool"), b"#!/bin/sh\n").unwrap();
    pacquet_fs::symlink_dir(&outside_bins, &modules_dir.join(".bin")).unwrap();

    let current_lockfile = lockfile_with_root_importer(ProjectSnapshot {
        dev_dependencies: Some(dep_map(&[("dev-dep", "1.0.0")])),
        ..ProjectSnapshot::default()
    });

    prune_direct_deps_excluded_by_groups(
        &current_lockfile,
        FULL,
        PROD_ONLY,
        workspace_root,
        config,
        None,
    )
    .expect("prune should succeed");

    assert!(outside_bins.join("devtool").exists());
    assert!(modules_dir.join("dev-dep").symlink_metadata().is_err());
}

/// A scoped alias whose `@scope/` component is a symlink must not be
/// unlinked through it.
#[test]
fn refuses_to_unlink_through_a_symlinked_scope_dir() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let modules_dir = workspace_root.join("node_modules");
    let outside_scope = workspace_root.join("outside-scope");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    fs::create_dir_all(&modules_dir).unwrap();
    fs::create_dir_all(&outside_scope).unwrap();
    let target = workspace_root.join("some-target");
    fs::create_dir_all(&target).unwrap();
    pacquet_fs::symlink_dir(&target, &outside_scope.join("dev-dep")).unwrap();
    pacquet_fs::symlink_dir(&outside_scope, &modules_dir.join("@scope")).unwrap();

    let current_lockfile = lockfile_with_root_importer(ProjectSnapshot {
        dev_dependencies: Some(dep_map(&[("@scope/dev-dep", "1.0.0")])),
        ..ProjectSnapshot::default()
    });

    prune_direct_deps_excluded_by_groups(
        &current_lockfile,
        FULL,
        PROD_ONLY,
        workspace_root,
        config,
        None,
    )
    .expect("prune should succeed");

    assert!(is_symlink_or_junction(&outside_scope.join("dev-dep")).unwrap());
}

/// The reverse switch (`--prod` → full) only widens the selection, so
/// nothing is removed.
#[test]
fn widening_the_selection_removes_nothing() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let modules_dir = workspace_root.join("node_modules");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    link_dep(&modules_dir, "keep-me", None);

    let current_lockfile = lockfile_with_root_importer(ProjectSnapshot {
        dependencies: Some(dep_map(&[("keep-me", "1.0.0")])),
        ..ProjectSnapshot::default()
    });

    prune_direct_deps_excluded_by_groups(
        &current_lockfile,
        PROD_ONLY,
        FULL,
        workspace_root,
        config,
        None,
    )
    .expect("prune should succeed");

    assert!(is_symlink_or_junction(&modules_dir.join("keep-me")).unwrap());
}

#[test]
fn prune_direct_deps_respects_trusted_importer_allow_set() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let selected_modules = workspace_root.join("packages/selected/node_modules");
    let unselected_modules = workspace_root.join("packages/unselected/node_modules");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = workspace_root.join("node_modules");
    config.virtual_store_dir = config.modules_dir.join(".pacquet");
    let config = config.leak();

    link_dep(&selected_modules, "selected-dev", None);
    link_dep(&unselected_modules, "unselected-dev", None);
    let mut current_lockfile = lockfile_with_root_importer(ProjectSnapshot::default());
    current_lockfile.importers = HashMap::from([
        (
            "packages/selected".to_string(),
            ProjectSnapshot {
                dev_dependencies: Some(dep_map(&[("selected-dev", "1.0.0")])),
                ..Default::default()
            },
        ),
        (
            "packages/unselected".to_string(),
            ProjectSnapshot {
                dev_dependencies: Some(dep_map(&[("unselected-dev", "1.0.0")])),
                ..Default::default()
            },
        ),
    ]);
    let prunable_importer_ids = HashSet::from(["packages/selected".to_string()]);

    prune_direct_deps_excluded_by_groups(
        &current_lockfile,
        FULL,
        PROD_ONLY,
        workspace_root,
        config,
        Some(&prunable_importer_ids),
    )
    .expect("prune should succeed");

    assert!(selected_modules.join("selected-dev").symlink_metadata().is_err());
    assert!(is_symlink_or_junction(&unselected_modules.join("unselected-dev")).unwrap());
}
