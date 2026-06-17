use super::get_hoisting_limits;
use pacquet_config::HoistingLimits;
use pacquet_lockfile::{
    Lockfile, PkgName, PkgVerPeer, ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec,
};
use std::collections::{BTreeSet, HashMap};

fn project_with_deps(names: &[&str]) -> ProjectSnapshot {
    let mut deps = ResolvedDependencyMap::new();
    for name in names {
        // `get_hoisting_limits` reads only the alias keys; the spec
        // value is filled in just to satisfy the map's value type.
        deps.insert(
            name.parse::<PkgName>().expect("valid pkg name"),
            ResolvedDependencySpec {
                specifier: "1.0.0".to_string(),
                version: "1.0.0".parse::<PkgVerPeer>().expect("parse version").into(),
            },
        );
    }
    ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() }
}

fn root_only() -> HashMap<String, ProjectSnapshot> {
    let mut importers = HashMap::new();
    importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), project_with_deps(&["a", "b"]));
    importers
}

#[test]
fn none_mode_yields_no_borders() {
    assert!(get_hoisting_limits(&root_only(), HoistingLimits::None).is_empty());
}

#[test]
fn root_direct_deps_are_bordered_under_dependencies_mode() {
    let limits = get_hoisting_limits(&root_only(), HoistingLimits::Dependencies);
    assert_eq!(limits.keys().cloned().collect::<Vec<_>>(), vec![".@".to_string()]);
    assert_eq!(limits[".@"], BTreeSet::from(["a".to_string(), "b".to_string()]));
}

#[test]
fn workspaces_mode_borders_packages_at_root() {
    let mut importers = HashMap::new();
    importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), project_with_deps(&["a"]));
    importers.insert("packages/foo".to_string(), project_with_deps(&["b"]));

    let limits = get_hoisting_limits(&importers, HoistingLimits::Workspaces);
    assert_eq!(limits.keys().cloned().collect::<Vec<_>>(), vec![".@".to_string()]);
    assert_eq!(limits[".@"], BTreeSet::from(["a".to_string(), "packages%2Ffoo".to_string()]));
}

#[test]
fn dependencies_mode_borders_each_importer() {
    let mut importers = HashMap::new();
    importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), project_with_deps(&["a"]));
    importers.insert("packages/foo".to_string(), project_with_deps(&["b"]));

    let limits = get_hoisting_limits(&importers, HoistingLimits::Dependencies);
    let mut keys = limits.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    assert_eq!(keys, vec![".@".to_string(), "packages%2Ffoo@workspace:packages/foo".to_string()]);
    assert_eq!(limits[".@"], BTreeSet::from(["a".to_string(), "packages%2Ffoo".to_string()]));
    assert_eq!(limits["packages%2Ffoo@workspace:packages/foo"], BTreeSet::from(["b".to_string()]));
}
