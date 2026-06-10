use super::{BuildSnapshotError, build_package_snapshot, registry_package_key};
use node_semver::Version;
use pacquet_lockfile::{LockfileResolution, PkgName, PkgVerPeer};
use pacquet_registry::{PackageDistribution, PackageVersion};
use pretty_assertions::assert_eq;
use ssri::Integrity;
use std::collections::HashMap;

fn integrity(text: &str) -> Integrity {
    text.parse().expect("parse integrity string")
}

fn make_package(name: &str, version: &str) -> PackageVersion {
    PackageVersion {
        name: name.to_string(),
        version: version.parse::<Version>().expect("parse semver"),
        dist: PackageDistribution {
            integrity: Some(integrity(
                "sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==",
            )),
            shasum: None,
            tarball: format!("https://registry.npmjs.org/{name}/-/{name}-{version}.tgz"),
            file_count: None,
            unpacked_size: None,
            attestations: None,
        },
        dependencies: None,
        dev_dependencies: None,
        peer_dependencies: None,
        optional_dependencies: None,
        peer_dependencies_meta: None,
        other: HashMap::default(),
        npm_user: None,
        deprecated: None,
    }
}

#[test]
fn builds_package_key_without_leading_slash_and_no_peer() {
    let pkg = make_package("react", "17.0.2");
    let key = registry_package_key(&pkg).unwrap();
    assert_eq!(key.to_string(), "react@17.0.2");
}

#[test]
fn builds_package_key_for_scoped_name() {
    let pkg = make_package("@types/node", "18.7.19");
    let key = registry_package_key(&pkg).unwrap();
    assert_eq!(key.to_string(), "@types/node@18.7.19");
}

#[test]
fn builds_metadata_with_registry_resolution_and_no_deps() {
    let pkg = make_package("lodash", "4.17.21");
    let built = build_package_snapshot(&pkg, &HashMap::new()).unwrap();

    assert_eq!(built.package_key.to_string(), "lodash@4.17.21");
    dbg!(&built.metadata.resolution);
    assert!(matches!(built.metadata.resolution, LockfileResolution::Registry(_)));
    dbg!(&built.snapshot.dependencies);
    assert!(built.snapshot.dependencies.is_none());
}

#[test]
fn builds_snapshot_with_resolved_dependencies() {
    let pkg = make_package("react-dom", "17.0.2");
    let mut resolved = HashMap::new();
    resolved.insert("react".to_string(), "17.0.2".parse::<PkgVerPeer>().unwrap());

    let built = build_package_snapshot(&pkg, &resolved).unwrap();

    let deps = built.snapshot.dependencies.expect("dependencies should be populated");
    assert_eq!(deps.len(), 1);
    let react_key = PkgName::parse("react").unwrap();
    assert_eq!(deps.get(&react_key).unwrap().to_string(), "17.0.2");
}

#[test]
fn returns_error_when_integrity_is_missing() {
    let mut pkg = make_package("broken", "1.0.0");
    pkg.dist.integrity = None;

    let err =
        build_package_snapshot(&pkg, &HashMap::new()).expect_err("should fail without integrity");
    eprintln!("err={err:?}");
    assert!(matches!(err, BuildSnapshotError::MissingIntegrity { .. }));
}
