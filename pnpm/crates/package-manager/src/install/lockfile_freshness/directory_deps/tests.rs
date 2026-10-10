use super::{
    LocalDepContext, SpecDirs, check_local_dep_group_freshness, dependency_manifests_by_dir,
    peers::{PeerShadowing, check_recorded_peer_specs_match},
    project_manifests_by_dir,
};
use crate::install::FreshnessCheckError;
use pnpm_catalogs_resolver::CatalogResolutionError;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::{PackageMetadata, StalenessReason};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[test]
fn unchanged_catalog_peer_specs_are_accepted_without_catalogs() {
    let catalogs = Catalogs::new();
    let result = check_peer_spec("catalog:", "catalog:", &catalogs);
    assert!(result.is_ok(), "raw recorded catalog peers must still match: {result:?}");
}

#[test]
fn a_changed_catalog_peer_without_catalogs_is_outdated() {
    let error = check_peer_spec("catalog:", "1.0.0", &Catalogs::new())
        .expect_err("the peer range changed without configured catalogs");
    assert!(
        matches!(
            &error,
            FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated { .. })
        ),
        "expected a stale local dependency, got {error:?}",
    );
}

#[test]
fn missing_catalog_peers_preserve_the_catalog_error() {
    let catalogs = Catalogs::from([(
        "default".to_string(),
        std::collections::BTreeMap::from([("foo".to_string(), "1.0.0".to_string())]),
    )]);
    let error = check_peer_spec("catalog:missing", "1.0.0", &catalogs)
        .expect_err("the named catalog does not exist");
    assert!(
        matches!(
            &error,
            FreshnessCheckError::InvalidCatalog(CatalogResolutionError::EntryNotFoundForSpec {
                alias,
                catalog_name,
            }) if alias == "foo" && catalog_name == "missing"
        ),
        "expected the catalog error, got {error:?}",
    );
}

#[test]
fn a_changed_catalog_peer_range_is_outdated() {
    let catalogs = Catalogs::from([(
        "default".to_string(),
        std::collections::BTreeMap::from([("foo".to_string(), "2.0.0".to_string())]),
    )]);
    let error = check_peer_spec("catalog:", "1.0.0", &catalogs)
        .expect_err("the catalog range differs from the recorded range");
    assert!(
        matches!(
            &error,
            FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated { .. })
        ),
        "expected a stale local dependency, got {error:?}",
    );
}

fn check_peer_spec(
    manifest_spec: &str,
    recorded_spec: &str,
    catalogs: &Catalogs,
) -> Result<(), FreshnessCheckError> {
    let dep = LocalDepContext {
        name: "lib",
        rel_path: "lib",
        dir: Path::new("lib"),
        dirs: SpecDirs {
            workspace_root: Path::new("."),
            lockfile_dir: Path::new("."),
            manifests_by_dir: &HashMap::new(),
        },
        catalogs,
    };
    let metadata: PackageMetadata = serde_json::from_value(serde_json::json!({
        "resolution": { "type": "directory", "directory": "lib" },
        "peerDependencies": { "foo": recorded_spec },
    }))
    .expect("parse local package metadata");
    check_recorded_peer_specs_match(&dep, &HashMap::from([("foo", manifest_spec)]), &metadata)
}

fn project(
    root_dir: &str,
    manifest: serde_json::Value,
    dependency_manifest: Option<serde_json::Value>,
) -> pnpm_workspace::Project {
    let root_dir = PathBuf::from(root_dir);
    let manifest_path = root_dir.join("package.json");
    pnpm_workspace::Project {
        manifest: PackageManifest::from_value(manifest_path.clone(), manifest),
        dependency_manifest: dependency_manifest.map(|value| {
            PackageManifest::from_value(manifest_path, value)
        }),
        root_dir,
    }
}

#[test]
fn injected_projects_are_read_through_their_dependency_manifests() {
    let projects = [
        project("/ws/app", serde_json::json!({ "name": "app" }), None),
        project(
            "/ws/lib",
            serde_json::json!({ "name": "lib" }),
            Some(serde_json::json!({ "name": "lib", "dependencies": { "foo": "1.0.0" } })),
        ),
    ];
    let dependency_manifests = dependency_manifests_by_dir(Some(&projects));
    let by_dir = project_manifests_by_dir(
        projects.iter().map(|project| &project.manifest),
        dependency_manifests.as_ref(),
    );

    let lib = by_dir[Path::new("/ws/lib")];
    assert_eq!(lib.value()["dependencies"], serde_json::json!({ "foo": "1.0.0" }));
    let app = by_dir[Path::new("/ws/app")];
    assert_eq!(app.value(), &serde_json::json!({ "name": "app" }));
}

#[test]
fn projects_without_dependency_manifests_have_none_to_index() {
    let projects = [project("/ws/app", serde_json::json!({ "name": "app" }), None)];
    assert!(dependency_manifests_by_dir(Some(&projects)).is_none());
    assert!(dependency_manifests_by_dir(None).is_none());
}

#[test]
fn freshness_workspace_packages_prefer_the_dependency_manifest() {
    let projects = [project(
        "/ws/lib",
        serde_json::json!({ "name": "lib", "version": "1.0.0" }),
        Some(serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "dependencies": { "foo": "1.0.0" },
        })),
    )];
    let dependency_manifests = dependency_manifests_by_dir(Some(&projects));
    let project_manifests = [(projects[0].root_dir.clone(), &projects[0].manifest)];

    let map = crate::install::workspace_state::build_workspace_packages_map_from_manifests(
        &project_manifests,
        dependency_manifests.as_ref(),
    );

    let lib = &map["lib"]["1.0.0"];
    assert_eq!(lib.manifest["dependencies"], serde_json::json!({ "foo": "1.0.0" }));
}

/// Check a directory dependency declaring `react` both as a dependency
/// (`19.2.7`) and as a peer (`^18.0.0`) against a lockfile that resolved it
/// to `locked` and recorded `recorded_peers` for the package.
fn check_dependency_that_is_also_a_peer(
    auto_install_peers: bool,
    recorded_peers: &serde_json::Value,
    locked: &str,
) -> Result<(), FreshnessCheckError> {
    let manifest = PackageManifest::from_value(
        PathBuf::from("/ws/lib/package.json"),
        serde_json::json!({
            "name": "lib",
            "dependencies": { "react": "19.2.7" },
            "peerDependencies": { "react": "^18.0.0" },
        }),
    );
    let metadata: PackageMetadata = serde_json::from_value(serde_json::json!({
        "resolution": { "type": "directory", "directory": "lib" },
        "peerDependencies": recorded_peers,
    }))
    .expect("parse local package metadata");
    let snapshot_deps = HashMap::from([(
        pnpm_lockfile::PkgName::parse("react").unwrap(),
        locked.parse::<pnpm_lockfile::SnapshotDepRef>().unwrap(),
    )]);
    let catalogs = Catalogs::new();
    let dep = LocalDepContext {
        name: "lib",
        rel_path: "lib",
        dir: Path::new("/ws/lib"),
        dirs: SpecDirs {
            workspace_root: Path::new("/ws"),
            lockfile_dir: Path::new("/ws"),
            manifests_by_dir: &HashMap::new(),
        },
        catalogs: &catalogs,
    };
    let peers = PeerShadowing::of(&manifest, &metadata, auto_install_peers);
    check_local_dep_group_freshness(
        &dep,
        &manifest,
        DependencyGroup::Prod,
        Some(&snapshot_deps),
        (false, &peers.shadowed),
    )?;
    peers.check_local_peer_deps_freshness(&dep, &manifest, &metadata)
}

#[test]
fn an_auto_installed_peer_supplies_the_dependency_it_shadows() {
    let peer = serde_json::json!({ "react": "^18.0.0" });
    let result = check_dependency_that_is_also_a_peer(true, &peer, "18.3.1");
    assert!(result.is_ok(), "the snapshot records the peer's resolution: {result:?}");
    // The peer range still counts: the lockfile must have recorded it.
    assert!(check_dependency_that_is_also_a_peer(true, &serde_json::json!({}), "18.3.1").is_err());
}

#[test]
fn a_peer_the_parent_provided_shadows_the_dependency_without_auto_install() {
    let peer = serde_json::json!({ "react": "^18.0.0" });
    let result = check_dependency_that_is_also_a_peer(false, &peer, "18.3.1");
    assert!(result.is_ok(), "the recorded peer shows the parent supplied it: {result:?}");
}

#[test]
fn an_unshadowed_dependency_is_compared_and_recorded_as_no_peer() {
    let no_peers = serde_json::json!({});
    let result = check_dependency_that_is_also_a_peer(false, &no_peers, "19.2.7");
    assert!(result.is_ok(), "the dependency resolved as a regular one: {result:?}");
    assert!(
        check_dependency_that_is_also_a_peer(false, &no_peers, "18.3.1").is_err(),
        "an unshadowed dependency must still satisfy its own spec",
    );
}
