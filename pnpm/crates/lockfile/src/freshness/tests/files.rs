use super::{
    Lockfile, StalenessReason, manifest_from_json, satisfies_package_manifest, text_block,
};

/// A `publishDirectory` that differs from the manifest's
/// `publishConfig.directory` surfaces as drift.
#[test]
fn publish_directory_mismatch_returns_publish_directory_mismatch() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    publishDirectory: ./dist"
        "    dependencies:"
        "      react:"
        "        specifier: ^17.0.2"
        "        version: 17.0.2"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "publishConfig": { "directory": "./build" },
        "dependencies": { "react": "^17.0.2" }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
        .expect_err("should be stale");
    assert!(
        matches!(err, StalenessReason::PublishDirectoryMismatch { .. }),
        "expected PublishDirectoryMismatch, got {err:?}",
    );
}

#[test]
fn link_directory_mismatch_returns_link_directory_mismatch() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    publishDirectory: ./dist"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "publishConfig": { "directory": "./dist", "linkDirectory": false }
    }"#,
    );
    let err = satisfies_package_manifest(importer, &manifest, true, &|_: &str| false)
        .expect_err("should be stale");
    assert!(
        matches!(err, StalenessReason::LinkDirectoryMismatch { .. }),
        "expected LinkDirectoryMismatch, got {err:?}",
    );
}

#[test]
fn publish_directory_match_satisfies() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    publishDirectory: ./dist"
        "    dependencies:"
        "      foo:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "publishConfig": { "directory": "./dist" },
        "dependencies": { "foo": "1.0.0" }
    }"#,
    );
    assert!(satisfies_package_manifest(importer, &manifest, true, &|_: &str| false).is_ok());
}
