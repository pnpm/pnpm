use std::collections::HashMap;

use pacquet_lockfile::{
    ComVer, ImporterDepVersion, Lockfile, LockfileResolution, LockfileVersion, PackageMetadata,
    PkgName, PkgNameVerPeer, PkgVerPeer, ProjectSnapshot, RegistryResolution,
    ResolvedDependencySpec, TarballResolution,
};

use super::{reusable_importer_dep, synthesize_reused_result};

fn single_dep_importer(alias: &str, resolved: &str) -> HashMap<String, ProjectSnapshot> {
    let mut deps = HashMap::new();
    deps.insert(
        alias.parse::<PkgName>().expect("parse alias"),
        ResolvedDependencySpec {
            specifier: resolved.to_string(),
            version: ImporterDepVersion::Regular(
                resolved.parse::<PkgVerPeer>().expect("parse version"),
            ),
        },
    );
    HashMap::from([(
        ".".to_string(),
        ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
    )])
}

fn empty_lockfile() -> Lockfile {
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfile v9"),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: HashMap::new(),
        packages: None,
        snapshots: None,
    }
}

fn registry_metadata() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
                .parse()
                .expect("parse integrity"),
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

#[test]
fn reuses_when_locked_version_satisfies_the_manifest_range() {
    let importers = single_dep_importer("react", "18.2.0");
    let key = reusable_importer_dep(&importers, ".", "react", "^18.0.0")
        .expect("locked 18.2.0 satisfies ^18.0.0");
    assert_eq!(key.to_string(), "react@18.2.0");
}

#[test]
fn reuses_across_a_widened_but_still_satisfied_range() {
    let importers = single_dep_importer("react", "18.2.0");
    assert!(reusable_importer_dep(&importers, ".", "react", ">=17").is_some());
}

#[test]
fn fresh_resolves_when_range_no_longer_satisfies_locked_version() {
    let importers = single_dep_importer("react", "18.2.0");
    assert!(reusable_importer_dep(&importers, ".", "react", "^19.0.0").is_none());
}

#[test]
fn fresh_resolves_a_new_dependency_absent_from_the_lockfile() {
    let importers = single_dep_importer("react", "18.2.0");
    assert!(reusable_importer_dep(&importers, ".", "left-pad", "^1.0.0").is_none());
}

#[test]
fn synthesizes_a_registry_resolution_with_the_recorded_integrity() {
    let key: PkgNameVerPeer = "react@18.2.0".parse().expect("parse key");
    let metadata = registry_metadata();
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata.clone())]));

    let result =
        synthesize_reused_result(&lockfile, &key, "react").expect("registry dep is reusable");
    assert_eq!(result.id.as_str(), "react@18.2.0");
    let name_ver = result.name_ver.expect("name_ver");
    assert_eq!(name_ver.name.to_string(), "react");
    assert_eq!(name_ver.suffix.to_string(), "18.2.0");
    assert_eq!(result.resolution, metadata.resolution);
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.alias.as_deref(), Some("react"));
    let manifest = result.manifest.expect("synthesized manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("react"));
    assert_eq!(manifest.get("version").and_then(serde_json::Value::as_str), Some("18.2.0"));
}

#[test]
fn synthesized_manifest_carries_peer_metadata() {
    let key: PkgNameVerPeer = "react-dom@18.2.0".parse().expect("parse key");
    let mut metadata = registry_metadata();
    metadata.peer_dependencies =
        Some(HashMap::from([("react".to_string(), "^18.0.0".to_string())]));
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata)]));

    let result =
        synthesize_reused_result(&lockfile, &key, "react-dom").expect("registry dep is reusable");
    let manifest = result.manifest.expect("synthesized manifest");
    let peers =
        manifest.get("peerDependencies").and_then(serde_json::Value::as_object).expect("peers");
    assert_eq!(peers.get("react").and_then(serde_json::Value::as_str), Some("^18.0.0"));
}

#[test]
fn synthesized_manifest_carries_deprecated_metadata() {
    let key: PkgNameVerPeer = "left-pad@1.3.0".parse().expect("parse key");
    let mut metadata = registry_metadata();
    metadata.deprecated = Some("use String.prototype.padStart()".to_string());
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata)]));

    let result =
        synthesize_reused_result(&lockfile, &key, "left-pad").expect("registry dep is reusable");
    let manifest = result.manifest.expect("synthesized manifest");
    assert_eq!(
        manifest.get("deprecated").and_then(serde_json::Value::as_str),
        Some("use String.prototype.padStart()"),
    );
}

#[test]
fn does_not_reuse_non_registry_resolutions() {
    let key: PkgNameVerPeer = "pkg-from-tarball@1.0.0".parse().expect("parse key");
    let mut metadata = registry_metadata();
    metadata.resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://example.test/pkg.tgz".to_string(),
        integrity: None,
        git_hosted: None,
        path: None,
    });
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata)]));

    assert!(synthesize_reused_result(&lockfile, &key, "pkg-from-tarball").is_none());
}

#[test]
fn does_not_reuse_a_package_absent_from_the_packages_map() {
    let key: PkgNameVerPeer = "react@18.2.0".parse().expect("parse key");
    let lockfile = empty_lockfile();
    assert!(synthesize_reused_result(&lockfile, &key, "react").is_none());
}

#[test]
fn does_not_reuse_a_non_semver_version_slot() {
    let key: PkgNameVerPeer =
        "pkg@https://example.test/pkg.tgz".parse().expect("parse url-keyed entry");
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), registry_metadata())]));
    assert!(synthesize_reused_result(&lockfile, &key, "pkg").is_none());
}

fn default_registry() -> HashMap<String, String> {
    HashMap::from([("default".to_string(), "https://registry.example.test/".to_string())])
}

#[test]
fn current_pkg_materializes_a_registry_resolution_into_its_tarball_url() {
    let key: PkgNameVerPeer = "react@18.2.0(foo@1.0.0)".parse().expect("parse key");
    let mut lockfile = empty_lockfile();
    lockfile.packages =
        Some(HashMap::from([("react@18.2.0".parse().expect("parse key"), registry_metadata())]));

    let current_pkg = super::current_pkg_from_lockfile(&lockfile, &key, &default_registry())
        .expect("packages entry exists");

    assert_eq!(current_pkg.id.to_string(), "react@18.2.0");
    assert_eq!(current_pkg.name.as_deref(), Some("react"));
    assert_eq!(current_pkg.version.as_deref(), Some("18.2.0"));
    let LockfileResolution::Tarball(tarball) = &current_pkg.resolution else {
        panic!("registry resolution must materialize as a tarball: {:?}", current_pkg.resolution);
    };
    assert_eq!(tarball.tarball, "https://registry.example.test/react/-/react-18.2.0.tgz");
    assert!(tarball.integrity.is_some(), "the recorded integrity carries over");
}

#[test]
fn current_pkg_routes_a_scoped_package_to_its_scope_registry() {
    let key: PkgNameVerPeer = "@scope/pkg@1.0.0".parse().expect("parse key");
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), registry_metadata())]));
    let mut registries = default_registry();
    registries.insert("@scope".to_string(), "https://scoped.example.test/".to_string());

    let current_pkg = super::current_pkg_from_lockfile(&lockfile, &key, &registries)
        .expect("packages entry exists");

    let LockfileResolution::Tarball(tarball) = &current_pkg.resolution else {
        panic!("registry resolution must materialize as a tarball");
    };
    assert_eq!(tarball.tarball, "https://scoped.example.test/@scope/pkg/-/pkg-1.0.0.tgz");
}

#[test]
fn current_pkg_passes_a_recorded_tarball_resolution_through() {
    let key: PkgNameVerPeer = "pkg@1.0.0".parse().expect("parse key");
    let mut metadata = registry_metadata();
    metadata.resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://example.test/pkg-1.0.0.tgz".to_string(),
        integrity: None,
        git_hosted: None,
        path: None,
    });
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), metadata)]));

    let current_pkg = super::current_pkg_from_lockfile(&lockfile, &key, &default_registry())
        .expect("packages entry exists");

    let LockfileResolution::Tarball(tarball) = &current_pkg.resolution else {
        panic!("tarball resolution must pass through");
    };
    assert_eq!(tarball.tarball, "https://example.test/pkg-1.0.0.tgz");
}

#[test]
fn current_pkg_is_none_without_a_packages_entry() {
    let key: PkgNameVerPeer = "react@18.2.0".parse().expect("parse key");
    let lockfile = empty_lockfile();
    assert!(super::current_pkg_from_lockfile(&lockfile, &key, &default_registry()).is_none());
}

#[test]
fn current_pkg_is_withheld_for_a_registry_entry_without_a_registry_map() {
    let key: PkgNameVerPeer = "react@18.2.0".parse().expect("parse key");
    let mut lockfile = empty_lockfile();
    lockfile.packages = Some(HashMap::from([(key.clone(), registry_metadata())]));
    assert!(super::current_pkg_from_lockfile(&lockfile, &key, &HashMap::new()).is_none());
}

#[test]
fn prior_child_key_applies_the_satisfies_gate() {
    let snapshot: pacquet_lockfile::SnapshotEntry =
        serde_json::from_value(serde_json::json!({ "dependencies": { "bar": "1.2.0" } }))
            .expect("parse snapshot entry");

    let key = super::prior_child_key(&snapshot, "bar", "^1.0.0").expect("recorded ref satisfies");
    assert_eq!(key.to_string(), "bar@1.2.0");

    assert!(
        super::prior_child_key(&snapshot, "bar", "^2.0.0").is_none(),
        "an edited range the recorded version no longer satisfies yields no prior key",
    );
    assert!(super::prior_child_key(&snapshot, "baz", "^1.0.0").is_none(), "unrecorded alias");
}
