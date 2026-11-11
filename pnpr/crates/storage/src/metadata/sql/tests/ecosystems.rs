use super::{Db, npm, packument, schema::execute, storage, write};
use crate::{
    DocumentWrite,
    metadata::{TagChange, TagUpdate},
};
use pnpr_error::RegistryError;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use serde_json::{Value, json};

fn cargo_document(name: &str, versions: &[(&str, bool)]) -> Vec<u8> {
    let entries: Vec<Value> = versions
        .iter()
        .map(|(version, yanked)| {
            json!({
                "name": name,
                "vers": version,
                "deps": [],
                "cksum": "1111111111111111111111111111111111111111111111111111111111111111",
                "features": {},
                "yanked": yanked,
            })
        })
        .collect();
    serde_json::to_vec(&json!({ "formatVersion": 1, "name": name, "versions": entries })).unwrap()
}

fn pypi_document(name: &str, files: &[(&str, &str)], deleted_files: &[&str]) -> Vec<u8> {
    let entries: Vec<Value> = files
        .iter()
        .map(|(filename, version)| {
            json!({
                "filename": filename,
                "version": version,
                "hashes": {
                    "sha256": "2222222222222222222222222222222222222222222222222222222222222222"
                },
                "size": 128,
                "yanked": false,
            })
        })
        .collect();
    serde_json::to_vec(&json!({
        "formatVersion": 2,
        "name": name,
        "files": entries,
        "deleted_files": deleted_files,
    }))
    .unwrap()
}

fn oci_document(name: &str, digests: &[&str], tags: &[(&str, &str)]) -> Vec<u8> {
    let manifests: Vec<Value> = digests
        .iter()
        .map(|digest| {
            json!({
                "digest": digest,
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "size": 256,
            })
        })
        .collect();
    let tag_entries: Vec<Value> = tags
        .iter()
        .map(|(tag, digest)| json!({ "tag": tag, "digest": digest, "updated": 300 }))
        .collect();
    serde_json::to_vec(&json!({
        "formatVersion": 1,
        "name": name,
        "manifests": manifests,
        "tags": tag_entries,
    }))
    .unwrap()
}

async fn cargo_versions_normalize_and_yank_in_place(db: Db) {
    let storage = storage(&db).for_hosted("acme", Ecosystem::Cargo);
    let name = CanonicalPackageName::parse("serde", Ecosystem::Cargo).unwrap();
    let initial = cargo_document("serde", &[("1.0.0", false), ("1.0.1", false)]);
    assert_eq!(
        storage.write_hosted_document_if_current(&name, &initial, None, None).await.unwrap(),
        DocumentWrite::Written,
    );
    let tag = TagChange { tag: "latest", target: Some("1.0.0"), principal: None };
    assert_eq!(storage.change_dist_tag(&name, tag).await.unwrap(), TagUpdate::NotFound);

    let current = storage
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    let yanked = cargo_document("serde", &[("1.0.0", true), ("1.0.1", false)]);
    assert_eq!(
        storage
            .write_hosted_document_if_current(&name, &yanked, Some(&current.version), None)
            .await
            .unwrap(),
        DocumentWrite::Written,
    );
    let rendered: Value = serde_json::from_slice(
        &storage
            .read_hosted_document(&name)
            .await
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(rendered["versions"][0]["yanked"], json!(true));
    assert_eq!(rendered["versions"][1]["yanked"], json!(false));

    let after_yank = storage
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    storage
        .write_hosted_document_if_current(&name, b"", Some(&after_yank.version), None)
        .await
        .unwrap();
    assert!(
        storage
            .read_hosted_document(&name)
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        storage
            .hosted_package_names()
            .await
            .unwrap()
            .is_empty(),
    );
}

async fn pypi_removed_filenames_remain_reserved_after_package_removal(db: Db) {
    let storage = storage(&db).for_hosted("acme", Ecosystem::Pypi);
    let name = CanonicalPackageName::parse("requests", Ecosystem::Pypi).unwrap();
    let initial = pypi_document(
        "requests",
        &[("requests-2.0.0.tar.gz", "2.0.0"), ("requests-2.1.0.tar.gz", "2.1.0")],
        &[],
    );
    storage.write_hosted_document_if_current(&name, &initial, None, None).await.unwrap();

    let current = storage
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    let after_delete = pypi_document(
        "requests",
        &[("requests-2.1.0.tar.gz", "2.1.0")],
        &["requests-2.0.0.tar.gz"],
    );
    storage
        .write_hosted_document_if_current(&name, &after_delete, Some(&current.version), None)
        .await
        .unwrap();

    let after = storage
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    let reupload = pypi_document(
        "requests",
        &[("requests-2.0.0.tar.gz", "2.0.0"), ("requests-2.1.0.tar.gz", "2.1.0")],
        &[],
    );
    let err =
        storage.write_hosted_document_if_current(&name, &reupload, Some(&after.version), None)
            .await;
    assert!(matches!(err, Err(RegistryError::BadRequest { .. })));

    assert!(storage.remove_package(&name, None).await.unwrap());
    let recreate = pypi_document("requests", &[("requests-2.0.0.tar.gz", "2.0.0")], &[]);
    let err = storage.write_hosted_document_if_current(&name, &recreate, None, None).await;
    assert!(matches!(err, Err(RegistryError::BadRequest { .. })));
}

async fn oci_manifests_and_tags_round_trip_and_clear(db: Db) {
    let storage = storage(&db).for_hosted("acme", Ecosystem::Oci);
    let name = CanonicalPackageName::parse("acme/app", Ecosystem::Oci).unwrap();
    let d1 = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    let d2 = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
    let initial = oci_document("acme/app", &[d1, d2], &[("latest", d1)]);
    storage.write_hosted_document_if_current(&name, &initial, None, None).await.unwrap();

    let current = storage
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    let updated = oci_document("acme/app", &[d2], &[("latest", d2)]);
    storage
        .write_hosted_document_if_current(&name, &updated, Some(&current.version), None)
        .await
        .unwrap();
    let rendered: Value = serde_json::from_slice(
        &storage
            .read_hosted_document(&name)
            .await
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        rendered["manifests"]
            .as_array()
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(rendered["tags"][0]["digest"], json!(d2));

    let after = storage
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    storage
        .write_hosted_document_if_current(&name, b"", Some(&after.version), None)
        .await
        .unwrap();
    assert!(
        storage
            .read_hosted_document(&name)
            .await
            .unwrap()
            .is_none(),
    );
}

async fn package_state_version_from_newer_format_is_refused(db: Db) {
    let npm_storage = storage(&db);
    write(&npm_storage, "pkg", &packument("pkg", &["1.0.0"])).await;
    let cargo_storage = storage(&db).for_hosted("acme", Ecosystem::Cargo);
    let cargo_name = CanonicalPackageName::parse("serde", Ecosystem::Cargo).unwrap();
    cargo_storage
        .write_hosted_document_if_current(
            &cargo_name,
            &cargo_document("serde", &[("1.0.0", false)]),
            None,
            None,
        )
        .await
        .unwrap();

    execute(&db, &["UPDATE packages SET state_version = 99"]).await;

    let npm_read = npm_storage.read_hosted_document(&npm("pkg")).await;
    assert!(matches!(npm_read, Err(RegistryError::DocumentFormatTooNew { .. })));
    let npm_write = npm_storage.write_hosted_document_if_current(
        &npm("pkg"),
        &serde_json::to_vec(&packument("pkg", &["1.0.0", "2.0.0"])).unwrap(),
        None,
        None,
    )
    .await;
    assert!(matches!(npm_write, Err(RegistryError::DocumentFormatTooNew { .. })));

    let cargo_read = cargo_storage.read_hosted_document(&cargo_name).await;
    assert!(matches!(cargo_read, Err(RegistryError::DocumentFormatTooNew { .. })));
    let cargo_write = cargo_storage.write_hosted_document_if_current(
        &cargo_name,
        &cargo_document("serde", &[("1.0.0", true)]),
        None,
        None,
    )
    .await;
    assert!(matches!(cargo_write, Err(RegistryError::DocumentFormatTooNew { .. })));
}

async fn cargo_yank_accepts_canonical_defaults_added_by_protocol_round_trip(db: Db) {
    let storage = storage(&db).for_hosted("acme", Ecosystem::Cargo);
    let name = CanonicalPackageName::parse("minimal", Ecosystem::Cargo).unwrap();
    let minimal = serde_json::to_vec(&json!({
        "formatVersion": 1,
        "name": "minimal",
        "versions": [{
            "name": "minimal",
            "vers": "0.1.0",
            "cksum": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "deps": [{ "name": "dep", "req": "^1.0", "target": null }],
            "links": null
        }]
    }))
    .unwrap();
    storage.write_hosted_document_if_current(&name, &minimal, None, None).await.unwrap();

    let current = storage
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    let round_tripped = serde_json::to_vec(&json!({
        "formatVersion": 1,
        "name": "minimal",
        "versions": [{
            "name": "minimal",
            "vers": "0.1.0",
            "cksum": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "deps": [{
                "name": "dep",
                "req": "^1.0",
                "features": [],
                "optional": false,
                "default_features": true,
                "kind": "normal"
            }],
            "features": {},
            "v": 1,
            "yanked": true
        }]
    }))
    .unwrap();
    assert_eq!(
        storage
            .write_hosted_document_if_current(&name, &round_tripped, Some(&current.version), None)
            .await
            .unwrap(),
        crate::DocumentWrite::Written,
    );
}

async fn pypi_yank_accepts_canonical_defaults_added_by_protocol_round_trip(db: Db) {
    let storage = storage(&db).for_hosted("acme", Ecosystem::Pypi);
    let name = CanonicalPackageName::parse("demo", Ecosystem::Pypi).unwrap();
    let minimal = serde_json::to_vec(&json!({
        "formatVersion": 2,
        "name": "demo",
        "files": [{
            "filename": "demo-0.1.0.tar.gz",
            "requires-python": null,
            "upload-time": null,
        }],
    }))
    .unwrap();
    storage.write_hosted_document_if_current(&name, &minimal, None, None).await.unwrap();

    let current = storage
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    let round_tripped = serde_json::to_vec(&json!({
        "formatVersion": 2,
        "name": "demo",
        "files": [{
            "filename": "demo-0.1.0.tar.gz",
            "hashes": {},
            "yanked": "security fix",
        }],
    }))
    .unwrap();
    assert_eq!(
        storage
            .write_hosted_document_if_current(&name, &round_tripped, Some(&current.version), None)
            .await
            .unwrap(),
        crate::DocumentWrite::Written,
    );
}

conformance!(
    cargo_versions_normalize_and_yank_in_place,
    cargo_yank_accepts_canonical_defaults_added_by_protocol_round_trip,
    pypi_removed_filenames_remain_reserved_after_package_removal,
    pypi_yank_accepts_canonical_defaults_added_by_protocol_round_trip,
    oci_manifests_and_tags_round_trip_and_clear,
    package_state_version_from_newer_format_is_refused,
);
