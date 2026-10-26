use super::reused_hoisted_locations;
use crate::{DependenciesGraph, DependenciesGraphNode, HoistedLocations, HoistedPackageMetadata};
use pnpm_lockfile::{DirectoryResolution, PkgIdWithPatchHash};
use pnpm_modules_yaml::DepPath;
use pretty_assertions::assert_eq;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

fn node(dep_path: &str, dir: PathBuf, present: bool) -> DependenciesGraphNode {
    DependenciesGraphNode {
        package: HoistedPackageMetadata {
            dep_path: DepPath::from(dep_path.to_string()),
            pkg_id_with_patch_hash: PkgIdWithPatchHash::from(dep_path),
            name: "pkg".to_string(),
            version: "1.0.0".to_string(),
            has_bin: false,
            has_bundled_dependencies: false,
            patch: None,
            resolution: DirectoryResolution { directory: "pkg".to_string() }.into(),
        },
        alias: None,
        modules: dir
            .parent()
            .expect("dir has a parent")
            .to_path_buf(),
        dir,
        optional: false,
        optional_dependencies: BTreeSet::new(),
        present,
        children: BTreeMap::new(),
    }
}

/// Locations are recorded with `/` on every platform, so this also covers
/// their lookup on Windows.
fn graph_with(lockfile_dir: &Path, nodes: &[(&str, &str, bool)]) -> DependenciesGraph {
    nodes
        .iter()
        .map(|&(dep_path, location, present)| {
            let dir = lockfile_dir.join(location);
            (dir.clone(), node(dep_path, dir, present))
        })
        .collect()
}

fn locations(entries: &[(&str, &[&str])]) -> HoistedLocations {
    entries
        .iter()
        .map(|(dep_path, dirs)| {
            (
                dep_path.to_string(),
                dirs.iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn every_location_reused_needs_no_rewrite() {
    let lockfile_dir = Path::new("/project");
    let recorded = locations(&[("a@1.0.0", &["node_modules/a", "node_modules/b/node_modules/a"])]);
    let graph = graph_with(
        lockfile_dir,
        &[("a@1.0.0", "node_modules/a", true), ("a@1.0.0", "node_modules/b/node_modules/a", true)],
    );
    assert_eq!(reused_hoisted_locations(&recorded, &graph, lockfile_dir), None);
}

#[test]
fn replaced_and_removed_locations_are_dropped() {
    let lockfile_dir = Path::new("/project");
    let recorded = locations(&[
        ("kept@1.0.0", &["node_modules/kept"]),
        ("reimported@1.0.0", &["node_modules/reimported"]),
        ("taken-over@1.0.0", &["node_modules/taken-over"]),
        ("removed@1.0.0", &["node_modules/removed"]),
        ("moved@1.0.0", &["node_modules/moved", "node_modules/x/node_modules/moved"]),
    ]);
    let graph = graph_with(
        lockfile_dir,
        &[
            ("kept@1.0.0", "node_modules/kept", true),
            ("reimported@1.0.0", "node_modules/reimported", false),
            ("taken-over@2.0.0", "node_modules/taken-over", true),
            ("moved@1.0.0", "node_modules/moved", true),
        ],
    );
    assert_eq!(
        reused_hoisted_locations(&recorded, &graph, lockfile_dir),
        Some(locations(&[
            ("kept@1.0.0", &["node_modules/kept"]),
            ("moved@1.0.0", &["node_modules/moved"]),
        ])),
    );
}
