use super::{
    LocalDepContext, SpecDirs, dependency_manifests_by_dir, peers::check_recorded_peer_specs_match,
    project_manifests_by_dir,
};
use crate::install::FreshnessCheckError;
use pnpm_catalogs_resolver::CatalogResolutionError;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::{PackageMetadata, StalenessReason};
use pnpm_package_manifest::PackageManifest;
use std::{
    collections::{HashMap, HashSet},
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
        declared_as_file: false,
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
    check_recorded_peer_specs_match(
        &dep,
        &HashMap::from([("foo", manifest_spec)]),
        &HashSet::from(["foo"]),
        &metadata,
    )
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
/// to `locked` and recorded `recorded_peers` for the package, for an importer
/// whose dependencies are `scope`.
fn check_dependency_that_is_also_a_peer(
    (auto_install_peers, scope): (bool, &[&str]),
    recorded_peers: &serde_json::Value,
    locked: &str,
) -> Result<(), FreshnessCheckError> {
    let manifest = serde_json::json!({
        "name": "lib",
        "dependencies": { "react": "19.2.7" },
        "peerDependencies": { "react": "^18.0.0" },
    });
    let snapshot_deps = HashMap::from([(
        pnpm_lockfile::PkgName::parse("react").unwrap(),
        locked.parse::<pnpm_lockfile::SnapshotDepRef>().unwrap(),
    )]);
    check_local_dep_peers((auto_install_peers, scope), manifest, recorded_peers, &snapshot_deps)
}

/// Check the production dependencies and peers of a directory dependency
/// with `manifest` against a lockfile that recorded `snapshot_deps` and
/// `recorded_peers` for it, for an importer whose dependencies are `scope`.
fn check_local_dep_peers(
    (auto_install_peers, scope): (bool, &[&str]),
    manifest: serde_json::Value,
    recorded_peers: &serde_json::Value,
    snapshot_deps: &HashMap<pnpm_lockfile::PkgName, pnpm_lockfile::SnapshotDepRef>,
) -> Result<(), FreshnessCheckError> {
    check_directory_dep_in_scope(
        &DirectoryDepCase {
            manifest,
            metadata: serde_json::json!({
                "resolution": { "type": "directory", "directory": "lib" },
                "peerDependencies": recorded_peers,
            }),
            snapshot: serde_json::json!({
                "dependencies": snapshot_deps
                    .iter()
                    .map(|(name, dep_ref)| (name.to_string(), serde_json::Value::String(dep_ref.to_string())))
                    .collect::<serde_json::Map<String, serde_json::Value>>(),
            }),
            frozen_lockfile: false,
            auto_install_peers,
            include_optional: true,
            catalogs: Catalogs::new(),
        },
        scope,
    )
}

/// A directory dependency `lib` at `/ws/lib` in a workspace at `/ws`.
struct DirectoryDepCase {
    manifest: serde_json::Value,
    metadata: serde_json::Value,
    snapshot: serde_json::Value,
    /// The allowance only `--frozen-lockfile` makes.
    frozen_lockfile: bool,
    auto_install_peers: bool,
    /// `false` for an install without optional dependencies.
    include_optional: bool,
    catalogs: Catalogs,
}

fn check_directory_dep(case: &DirectoryDepCase) -> Result<(), FreshnessCheckError> {
    check_directory_dep_in_scope(case, &[])
}

/// [`check_directory_dep`] for an importer whose dependencies are `scope`.
fn check_directory_dep_in_scope(
    case: &DirectoryDepCase,
    scope: &[&str],
) -> Result<(), FreshnessCheckError> {
    let importer: pnpm_lockfile::ProjectSnapshot = serde_json::from_value(serde_json::json!({
        "dependencies": scope
            .iter()
            .map(|name| (name.to_string(), serde_json::json!({ "specifier": "*", "version": "1.0.0" })))
            .collect::<serde_json::Map<String, serde_json::Value>>(),
    }))
    .expect("parse the importer");
    let dep_dir = PathBuf::from("/ws/lib");
    let local_manifest =
        PackageManifest::from_value(dep_dir.join("package.json"), case.manifest.clone());
    let metadata: PackageMetadata =
        serde_json::from_value(case.metadata.clone()).expect("parse local package metadata");
    let snapshot: pnpm_lockfile::SnapshotEntry =
        serde_json::from_value(case.snapshot.clone()).expect("parse the snapshot");
    let lockfile: pnpm_lockfile::Lockfile =
        serde_saphyr::from_str("lockfileVersion: '9.0'\nimporters: {}\n").expect("parse lockfile");
    let mut config = pnpm_config::Config::new();
    config.auto_install_peers = case.auto_install_peers;
    config.optional = case.include_optional;
    let ignored = pnpm_matcher::create_matcher(&[]);
    let manifests_by_dir = HashMap::from([(pnpm_fs::lexical_normalize(&dep_dir), &local_manifest)]);
    let catalogs = &case.catalogs;
    let check = crate::install::lockfile_freshness::ImporterSatisfactionCheck {
        lockfile: &lockfile,
        lockfile_dir: Path::new("/ws"),
        manifest: &local_manifest,
        importer_id: ".",
        config: &config,
        workspace: crate::install::lockfile_freshness::WorkspaceProjects {
            packages: None,
            manifests_by_dir: &manifests_by_dir,
            catalogs,
        },
        optional_exclusions: crate::install::lockfile_freshness::OptionalDependencyExclusions {
            ignored: &ignored,
            frozen_lockfile: case.frozen_lockfile,
        },
        parsed_overrides: None,
    };
    let dep = LocalDepContext {
        name: "lib",
        declared_as_file: false,
        rel_path: "lib",
        dir: &dep_dir,
        dirs: SpecDirs {
            workspace_root: Path::new("/ws"),
            lockfile_dir: Path::new("/ws"),
            manifests_by_dir: &HashMap::new(),
        },
        catalogs,
    };
    let mut visited = std::collections::HashSet::new();
    super::check_single_directory_dep_freshness(
        &check,
        |name| super::importer_declares(&importer, name),
        &dep,
        (Some(&snapshot), &metadata),
        &mut visited,
    )
}

#[test]
fn an_auto_installed_peer_supplies_the_dependency_it_shadows() {
    let peer = serde_json::json!({ "react": "^18.0.0" });
    let result = check_dependency_that_is_also_a_peer((true, &[]), &peer, "18.3.1");
    assert!(result.is_ok(), "the snapshot records the peer's resolution: {result:?}");
    // The peer range still counts: the lockfile must have recorded it.
    assert!(
        check_dependency_that_is_also_a_peer((true, &[]), &serde_json::json!({}), "18.3.1")
            .is_err(),
    );
}

/// The resolver reads a dependency whose value is not a string as no child,
/// but still as the package's own name, so it records neither a dependency
/// nor a peer for it.
#[test]
fn a_non_string_dependency_still_keeps_its_name_out_of_the_peers() {
    let manifest = serde_json::json!({
        "name": "lib",
        "dependencies": { "react": true },
        "peerDependencies": { "react": "^18.0.0" },
    });
    let result =
        check_local_dep_peers((false, &[]), manifest, &serde_json::json!({}), &HashMap::new());
    assert!(result.is_ok(), "the lockfile the resolver writes is fresh: {result:?}");
}

#[test]
fn a_peer_the_importer_provides_shadows_the_dependency_without_auto_install() {
    let peer = serde_json::json!({ "react": "^18.0.0" });
    let result = check_dependency_that_is_also_a_peer((false, &["react"]), &peer, "18.3.1");
    assert!(result.is_ok(), "the importer supplied the peer: {result:?}");
}

#[test]
fn an_unshadowed_dependency_is_compared_against_its_own_spec() {
    let peer = serde_json::json!({ "react": "^18.0.0" });
    let result = check_dependency_that_is_also_a_peer((false, &[]), &peer, "19.2.7");
    assert!(result.is_ok(), "the dependency resolved as a regular one: {result:?}");
    assert!(
        check_dependency_that_is_also_a_peer((false, &[]), &peer, "18.3.1").is_err(),
        "an unshadowed dependency must still satisfy its own spec",
    );
}

/// A lockfile written before every declared peer was recorded left out the
/// ones the package resolved as its own dependencies.
#[test]
fn an_unshadowed_peer_may_be_missing_from_the_recorded_peers() {
    let result =
        check_dependency_that_is_also_a_peer((false, &[]), &serde_json::json!({}), "19.2.7");
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn a_recorded_peer_the_manifest_does_not_declare_is_stale() {
    let peers = serde_json::json!({ "react": "^18.0.0", "vue": "^3.0.0" });
    assert!(check_dependency_that_is_also_a_peer((false, &[]), &peers, "19.2.7").is_err());
}

/// Check a directory dependency whose own manifest declares `is-positive`
/// under `manifest_key`, against a lockfile whose snapshot of it resolved
/// nothing.
fn check_dep_with_an_unresolved_entry(
    manifest_key: &str,
    frozen_lockfile: bool,
) -> Result<(), FreshnessCheckError> {
    let mut manifest = serde_json::json!({ "name": "lib" });
    manifest[manifest_key] = serde_json::json!({ "is-positive": "1.0.0" });
    check_directory_dep(&DirectoryDepCase {
        manifest,
        metadata: serde_json::json!({ "resolution": { "type": "directory", "directory": "lib" } }),
        snapshot: serde_json::json!({}),
        frozen_lockfile,
        auto_install_peers: true,
        include_optional: true,
        catalogs: Catalogs::new(),
    })
}

#[test]
fn a_directory_dependency_whose_dependency_went_unresolved_is_outdated() {
    assert!(
        check_dep_with_an_unresolved_entry("dependencies", false).is_err(),
        "a dependency the lockfile never resolved leaves the local dependency stale",
    );
    assert!(
        check_dep_with_an_unresolved_entry("dependencies", true).is_err(),
        "the frozen-install allowance covers optional dependencies only",
    );
}

#[test]
fn an_unresolved_optional_of_a_directory_dependency_is_satisfied_only_when_frozen() {
    assert!(
        check_dep_with_an_unresolved_entry("optionalDependencies", false).is_err(),
        "a resolving install retries the optional dependency",
    );
    let frozen = check_dep_with_an_unresolved_entry("optionalDependencies", true);
    assert!(
        frozen.is_ok(),
        "a frozen install keeps what the lockfile could not resolve: {frozen:?}",
    );
}

fn directory_metadata() -> serde_json::Value {
    serde_json::json!({ "resolution": { "type": "directory", "directory": "lib" } })
}

/// The snapshot records a child in the map its optionality puts it in, so a
/// child that became optional with the same version is a change.
#[test]
fn a_dependency_that_became_optional_is_outdated() {
    let result = check_directory_dep(&DirectoryDepCase {
        manifest: serde_json::json!({
            "name": "lib",
            "optionalDependencies": { "is-positive": "1.0.0" },
        }),
        metadata: directory_metadata(),
        snapshot: serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }),
        frozen_lockfile: false,
        auto_install_peers: true,
        include_optional: true,
        catalogs: Catalogs::new(),
    });
    assert!(result.is_err(), "the child moved to optionalDependencies: {result:?}");
}

#[test]
fn an_install_without_optional_dependencies_ignores_the_recorded_optional_children() {
    let result = check_directory_dep(&DirectoryDepCase {
        manifest: serde_json::json!({ "name": "lib" }),
        metadata: directory_metadata(),
        snapshot: serde_json::json!({ "optionalDependencies": { "is-positive": "1.0.0" } }),
        frozen_lockfile: false,
        auto_install_peers: true,
        include_optional: false,
        catalogs: Catalogs::new(),
    });
    assert!(result.is_ok(), "the optional map is not compared: {result:?}");
}

#[test]
fn a_misconfigured_catalog_child_keeps_the_catalog_error() {
    let error = check_directory_dep(&DirectoryDepCase {
        manifest: serde_json::json!({
            "name": "lib",
            "dependencies": { "is-positive": "catalog:missing" },
        }),
        metadata: directory_metadata(),
        snapshot: serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }),
        frozen_lockfile: false,
        auto_install_peers: true,
        include_optional: true,
        catalogs: Catalogs::from([(
            "default".to_string(),
            std::collections::BTreeMap::from([("is-positive".to_string(), "1.0.0".to_string())]),
        )]),
    })
    .expect_err("the named catalog does not exist");
    assert!(
        matches!(&error, FreshnessCheckError::InvalidCatalog(_)),
        "expected the catalog error, got {error:?}",
    );
}

/// The resolver drops a dependency that the package's own peer supplies
/// before it resolves `catalog:` specs, so a missing catalog entry on it is
/// no error.
#[test]
fn a_peer_shadowed_dependency_is_not_resolved_through_the_catalogs() {
    let result = check_directory_dep(&DirectoryDepCase {
        manifest: serde_json::json!({
            "name": "lib",
            "dependencies": { "react": "catalog:missing" },
            "peerDependencies": { "react": "^18.0.0" },
        }),
        metadata: serde_json::json!({
            "resolution": { "type": "directory", "directory": "lib" },
            "peerDependencies": { "react": "^18.0.0" },
        }),
        snapshot: serde_json::json!({ "dependencies": { "react": "18.3.1" } }),
        frozen_lockfile: false,
        auto_install_peers: true,
        include_optional: true,
        catalogs: Catalogs::from([(
            "default".to_string(),
            std::collections::BTreeMap::from([("is-positive".to_string(), "1.0.0".to_string())]),
        )]),
    });
    assert!(result.is_ok(), "the shadowed dependency is the peer check's: {result:?}");
}

/// A child that became optional is a change even when the install leaves
/// optional dependencies out: the lockfile still records it as required.
#[test]
fn a_dependency_that_became_optional_is_outdated_without_optional_dependencies() {
    let result = check_directory_dep(&DirectoryDepCase {
        manifest: serde_json::json!({
            "name": "lib",
            "optionalDependencies": { "is-positive": "1.0.0" },
        }),
        metadata: directory_metadata(),
        snapshot: serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }),
        frozen_lockfile: false,
        auto_install_peers: true,
        include_optional: false,
        catalogs: Catalogs::new(),
    });
    assert!(result.is_err(), "the lockfile records the child as required: {result:?}");
}

/// A frozen install keeps an optional child the lockfile could not resolve,
/// but one the lockfile records as required has changed.
#[test]
fn a_dependency_that_became_optional_is_outdated_on_a_frozen_install() {
    let result = check_directory_dep(&DirectoryDepCase {
        manifest: serde_json::json!({
            "name": "lib",
            "optionalDependencies": { "is-positive": "1.0.0" },
        }),
        metadata: directory_metadata(),
        snapshot: serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }),
        frozen_lockfile: true,
        auto_install_peers: true,
        include_optional: true,
        catalogs: Catalogs::new(),
    });
    assert!(result.is_err(), "the lockfile records the child as required: {result:?}");
}

/// The lockfile does not record the spec a tarball child was resolved from,
/// so only a frozen install keeps the recorded resolution.
#[test]
fn a_tarball_child_is_resolved_again_unless_the_install_is_frozen() {
    let check = |frozen_lockfile| {
        check_directory_dep(&DirectoryDepCase {
            manifest: serde_json::json!({
                "name": "lib",
                "dependencies": { "is-positive": "https://example.com/is-positive-3.1.0.tgz" },
            }),
            metadata: directory_metadata(),
            snapshot: serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }),
            frozen_lockfile,
            auto_install_peers: true,
            include_optional: true,
            catalogs: Catalogs::new(),
        })
    };
    assert!(check(false).is_err(), "a resolving install resolves the tarball again");
    assert!(check(true).is_ok(), "a frozen install keeps the recorded resolution");
}

/// A directory without a manifest is described by its name, as the resolver
/// describes it, but one whose manifest fails to read is not described at all.
#[test]
fn only_a_directory_without_a_manifest_is_described_by_its_name() {
    let tmp = tempfile::tempdir().expect("create a temporary directory");
    let dir = tmp.path().join("pkg");
    std::fs::create_dir(&dir).expect("create the dependency directory");
    let manifests_by_dir = HashMap::new();
    let describe = || {
        super::unhooked_manifest(&super::InjectedDirectory {
            dir: &dir,
            declared_as_file: true,
            workspace_root: tmp.path(),
            manifests_by_dir: &manifests_by_dir,
        })
    };
    let named = describe().expect("a directory without a manifest is described");
    assert_eq!(named.value()["name"], "pkg");
    std::fs::write(dir.join("package.json"), "{").expect("write a malformed package.json");
    assert!(describe().is_none(), "a malformed manifest is not a missing one");
}

#[test]
fn publish_config_directory_manifest_follows_declaration_kind() {
    let tmp = tempfile::tempdir().expect("create a temporary directory");
    let workspace_root = tmp.path().join("ws");
    let ws_pkg = workspace_root.join("packages/lib");
    let ws_dist = ws_pkg.join("dist");
    std::fs::create_dir_all(&ws_dist).expect("create workspace publish directory");
    std::fs::write(
        ws_pkg.join("package.json"),
        r#"{"name":"lib","version":"1.0.0","publishConfig":{"directory":"dist"},"dependencies":{"foo":"1.0.0"}}"#,
    )
    .expect("write workspace project manifest");
    std::fs::write(ws_dist.join("package.json"), r#"{"name":"lib","version":"1.0.0"}"#)
        .expect("write built dist manifest");

    let outside_pkg = tmp.path().join("outside");
    let outside_dist = outside_pkg.join("dist");
    std::fs::create_dir_all(&outside_dist).expect("create outside publish directory");
    std::fs::write(
        outside_pkg.join("package.json"),
        r#"{"name":"outside","version":"2.0.0","publishConfig":{"directory":"dist"}}"#,
    )
    .expect("write outside project manifest");

    let manifests_by_dir = HashMap::new();
    let ws_injected = super::unhooked_manifest(&super::InjectedDirectory {
        dir: &ws_dist,
        declared_as_file: false,
        workspace_root: &workspace_root,
        manifests_by_dir: &manifests_by_dir,
    })
    .expect("workspace publish directory resolves to parent project manifest");
    assert_eq!(ws_injected.value()["dependencies"]["foo"], "1.0.0");

    let outside_file = super::unhooked_manifest(&super::InjectedDirectory {
        dir: &outside_dist,
        declared_as_file: true,
        workspace_root: &workspace_root,
        manifests_by_dir: &manifests_by_dir,
    })
    .expect("file: dependency outside workspace resolves to parent publish manifest");
    assert_eq!(outside_file.value()["version"], "2.0.0");
}

#[test]
fn a_transitive_directory_dependency_is_checked_from_its_consumer() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let ws = tmp.path();
    let dir_a = ws.join("packages/a");
    let dir_b = ws.join("packages/b");
    let dir_c = ws.join("packages/c");
    std::fs::create_dir_all(&dir_a).expect("create packages/a");
    std::fs::create_dir_all(&dir_b).expect("create packages/b");
    std::fs::create_dir_all(&dir_c).expect("create packages/c");
    let manifest_a = PackageManifest::from_value(
        dir_a.join("package.json"),
        serde_json::json!({
            "name": "@ws/a",
            "version": "1.0.0",
            "dependencies": { "foo": "2.0.0" },
        }),
    );
    let manifest_b = PackageManifest::from_value(
        dir_b.join("package.json"),
        serde_json::json!({
            "name": "@ws/b",
            "version": "1.0.0",
            "dependencies": { "@ws/a": "workspace:*" },
        }),
    );
    let manifest_c = PackageManifest::from_value(
        dir_c.join("package.json"),
        serde_json::json!({
            "name": "@ws/c",
            "version": "1.0.0",
            "dependencies": { "@ws/b": "workspace:*" },
        }),
    );
    let lockfile: pnpm_lockfile::Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  packages/c:
    dependencies:
      '@ws/b':
        specifier: workspace:*
        version: file:packages/b
packages:
  '@ws/b@file:packages/b':
    resolution: { type: directory, directory: packages/b }
  '@ws/a@file:packages/a':
    resolution: { type: directory, directory: packages/a }
  foo@1.0.0:
    resolution: { integrity: sha512-foo }
snapshots:
  '@ws/b@file:packages/b':
    dependencies:
      '@ws/a': file:packages/a
  '@ws/a@file:packages/a':
    dependencies:
      foo: 1.0.0
  foo@1.0.0: {}
",
    )
    .expect("parse lockfile");
    let mut config = pnpm_config::Config::new();
    config.inject_workspace_packages = true;
    let ignored = pnpm_matcher::create_matcher(&[]);
    let manifests_by_dir = HashMap::from([
        (pnpm_fs::lexical_normalize(&dir_a), &manifest_a),
        (pnpm_fs::lexical_normalize(&dir_b), &manifest_b),
        (pnpm_fs::lexical_normalize(&dir_c), &manifest_c),
    ]);
    let catalogs = std::collections::BTreeMap::new();
    let check = crate::install::lockfile_freshness::ImporterSatisfactionCheck {
        lockfile: &lockfile,
        lockfile_dir: ws,
        manifest: &manifest_c,
        importer_id: "packages/c",
        config: &config,
        workspace: crate::install::lockfile_freshness::WorkspaceProjects {
            packages: None,
            manifests_by_dir: &manifests_by_dir,
            catalogs: &catalogs,
        },
        optional_exclusions: crate::install::lockfile_freshness::OptionalDependencyExclusions {
            ignored: &ignored,
            frozen_lockfile: false,
        },
        parsed_overrides: None,
    };
    let importer_c = lockfile.importers.get("packages/c").expect("packages/c importer");
    let err = super::check_directory_dependencies_freshness(&check, importer_c)
        .expect_err(
            "transitive file:packages/a dependency drift should be detected from packages/c",
        );
    assert!(
        matches!(
            err,
            crate::install::lockfile_freshness::FreshnessCheckError::Stale(
                pnpm_lockfile::StalenessReason::LocalDependencyOutdated { ref name, ref path }
            ) if name == "@ws/a" && path == "packages/a"
        ),
        "expected LocalDependencyOutdated for @ws/a at packages/a, got: {err:?}",
    );
}
