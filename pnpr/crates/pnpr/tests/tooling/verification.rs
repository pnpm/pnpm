use super::{create_test_config, seed_archive, seed_document};
use pnpr::verify::{VerifyOptions, verify_storage};
use pnpr_storage::{
    BlobId, Ecosystem,
    layout::{local_archive_path, write_local_layout_marker},
};
use tempfile::TempDir;

#[tokio::test]
async fn corrupted_attestations_are_reported_without_stopping_the_audit() {
    let dir = TempDir::new().unwrap();
    let (config, _config_file) = create_test_config(dir.path());
    let hosted = &config.storage.hosted_dir;
    write_local_layout_marker(hosted).unwrap();
    for name in ["first-package", "second-package"] {
        let document = serde_json::json!({
            "name": name,
            "versions": { "1.0.0": { "name": name, "version": "1.0.0", "dist": {
                "tarball": format!("http://localhost/{name}/-/{name}-1.0.0.tgz"),
                "attestations": { "url": "http://localhost/attestations" }
            } } }
        });
        seed_document(hosted, Ecosystem::Npm, name, &serde_json::to_vec(&document).unwrap());
        seed_archive(hosted, Ecosystem::Npm, name, &format!("{name}-1.0.0.tgz"), b"tarball");
        seed_archive(
            hosted,
            Ecosystem::Npm,
            name,
            &format!("{name}-1.0.0.sigstore"),
            name.as_bytes(),
        );
    }
    let key = BlobId::of_bytes(Ecosystem::Npm, b"first-package").key(1);
    let path = local_archive_path(hosted, "", Ecosystem::Npm, &key);
    std::fs::write(path, b"corrupted").unwrap();
    let report = verify_storage(&config, &VerifyOptions { check_hashes: true })
        .await
        .unwrap();
    assert_eq!(report.packages_checked, 2);
    assert_eq!(report.blobs_checked, 4);
    assert!(report.missing_blobs.is_empty());
    assert_eq!(report.corrupted_blobs.len(), 1);
    assert!(report.corrupted_blobs[0].contains("first-package-1.0.0.sigstore"));
}
