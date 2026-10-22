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

#[test]
fn rejects_modifying_dist_fields() {
    let pkg = CanonicalPackageName::parse("mypkg", pnpr_package_name::Ecosystem::Npm).unwrap();
    let hosted = json!({
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-test",
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                    "fileCount": 42,
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
                    "fileCount": 99,
                }
            }
        }
    });

    let err = enforce_published_version_immutability(&hosted, &pkg, &mut incoming).unwrap();
    assert!(matches!(err, RegistryError::BadRequest { .. }));
}

#[test]
fn restores_omitted_dist_fields() {
    let pkg = CanonicalPackageName::parse("mypkg", pnpr_package_name::Ecosystem::Npm).unwrap();
    let hosted = json!({
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-test",
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                    "fileCount": 42,
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
    assert_eq!(incoming["versions"]["1.0.0"]["dist"]["fileCount"], json!(42));
}

#[test]
fn rejects_missing_or_non_object_versions() {
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

    let mut incoming_empty = json!({});
    let err = enforce_published_version_immutability(&hosted, &pkg, &mut incoming_empty).unwrap();
    assert!(matches!(err, RegistryError::BadRequest { .. }));

    let mut incoming_null = json!({ "versions": null });
    let err = enforce_published_version_immutability(&hosted, &pkg, &mut incoming_null).unwrap();
    assert!(matches!(err, RegistryError::BadRequest { .. }));
}

#[test]
fn rejects_non_string_or_modified_shasum() {
    let pkg = CanonicalPackageName::parse("mypkg", pnpr_package_name::Ecosystem::Npm).unwrap();
    let hosted = json!({
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-test",
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                    "shasum": "abcd",
                }
            }
        }
    });

    let mut incoming_null_shasum = json!({
        "versions": {
            "1.0.0": {
                "name": "mypkg",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                    "shasum": null,
                }
            }
        }
    });
    let err =
        enforce_published_version_immutability(&hosted, &pkg, &mut incoming_null_shasum).unwrap();
    assert!(matches!(err, RegistryError::BadRequest { .. }));
}

#[test]
fn strips_round_tripped_dist_pnpr_overlay() {
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
                "dist": {
                    "integrity": "sha512-test",
                    "tarball": "http://localhost/mypkg/-/mypkg-1.0.0.tgz",
                    "pnpr": { "requiresBuild": false },
                }
            }
        }
    });

    assert!(enforce_published_version_immutability(&hosted, &pkg, &mut incoming).is_none());
    assert!(incoming["versions"]["1.0.0"]["dist"].get("pnpr").is_none());
}
