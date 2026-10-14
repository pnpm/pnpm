use super::{CanonicalPackageName, RegistryError, enforce_published_version_immutability};
use serde_json::json;

#[test]
fn allows_modifying_or_adding_deprecated_field() {
    let pkg = CanonicalPackageName::parse("mypkg", pnpr_package_name::Ecosystem::Npm).unwrap();
    let hosted = json!({
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "dependencies": { "dep": "1.0.0" },
                "dist": {
                    "integrity": "sha512-test",
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                }
            }
        }
    });

    let mut incoming = json!({
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "deprecated": "use 2.0.0 instead",
                "dependencies": { "dep": "1.0.0" },
                "dist": {
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                }
            }
        }
    });

    assert!(enforce_published_version_immutability(&hosted, &pkg, &mut incoming).is_none());
    assert_eq!(incoming["versions"]["1.0.0"]["dist"]["integrity"], json!("sha512-test"));
}

#[test]
fn rejects_mutating_dependencies_of_published_version() {
    let pkg = CanonicalPackageName::parse("mypkg", pnpr_package_name::Ecosystem::Npm).unwrap();
    let hosted = json!({
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "dependencies": { "dep": "1.0.0" },
                "dist": {
                    "integrity": "sha512-test",
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                }
            }
        }
    });

    let mut incoming = json!({
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "dependencies": { "dep": "2.0.0" },
                "dist": {
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                }
            }
        }
    });

    let err = enforce_published_version_immutability(&hosted, &pkg, &mut incoming).unwrap();
    assert!(matches!(err, RegistryError::BadRequest { .. }));
}

#[test]
fn rejects_adding_scripts_to_published_version() {
    let pkg = CanonicalPackageName::parse("mypkg", pnpr_package_name::Ecosystem::Npm).unwrap();
    let hosted = json!({
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-test",
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                }
            }
        }
    });

    let mut incoming = json!({
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "scripts": { "postinstall": "malicious" },
                "dist": {
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                }
            }
        }
    });

    let err = enforce_published_version_immutability(&hosted, &pkg, &mut incoming).unwrap();
    assert!(matches!(err, RegistryError::BadRequest { .. }));
}

#[test]
fn restores_omitted_metadata_fields() {
    let pkg = CanonicalPackageName::parse("mypkg", pnpr_package_name::Ecosystem::Npm).unwrap();
    let hosted = json!({
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "description": "original description",
                "_npmUser": { "name": "alice" },
                "dist": {
                    "integrity": "sha512-test",
                    "shasum": "abcd",
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                }
            }
        }
    });

    let mut incoming = json!({
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                }
            }
        }
    });

    assert!(enforce_published_version_immutability(&hosted, &pkg, &mut incoming).is_none());
    assert_eq!(incoming["versions"]["1.0.0"]["description"], json!("original description"));
    assert_eq!(incoming["versions"]["1.0.0"]["_npmUser"]["name"], json!("alice"));
    assert_eq!(incoming["versions"]["1.0.0"]["dist"]["integrity"], json!("sha512-test"));
    assert_eq!(incoming["versions"]["1.0.0"]["dist"]["shasum"], json!("abcd"));
}
