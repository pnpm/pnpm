use super::{RevisionBackfillReport, backfill_hosted_revision_refs};
use crate::{
    config::{Config, HostedStoreConfig},
    package_name::PackageName,
    storage::{Storage, TarballFinalize},
};
use object_store::memory::InMemory;
use pnpm_crypto_hash::integrity_addressed_tarball_path;
use serde_json::{Value, json};
use ssri::{Algorithm, IntegrityOpts};
use std::{net::SocketAddr, sync::Arc};
use tempfile::TempDir;
use tokio::fs;

fn config_in(tmp: &TempDir) -> Config {
    Config::static_serve(
        "127.0.0.1:7677".parse::<SocketAddr>().unwrap(),
        tmp.path().join("storage"),
    )
}

fn sri(bytes: &[u8], algorithm: Algorithm) -> String {
    let mut opts = IntegrityOpts::new().algorithm(algorithm);
    opts.input(bytes);
    opts.result().to_string()
}

async fn seed_version(config: &Config, package: &str, version: &str, dist: Value, tarball: &[u8]) {
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone());
    let package = PackageName::parse(package).unwrap();
    let packument = serde_json::to_vec(&json!({
        "name": package.as_str(),
        "versions": {
            (version): {
                "name": package.as_str(),
                "version": version,
                "dist": dist,
            }
        }
    }))
    .unwrap();
    storage.write_hosted_packument_if_current(&package, &packument, None).await.unwrap();
    let filename = package.tarball_name_for_version(version);
    let slot = storage.reserve_hosted_tarball(&package, &filename).await.unwrap();
    fs::write(&slot.tmp_path, tarball).await.unwrap();
    assert_eq!(storage.finalize_tarball_slot(slot).await.unwrap(), TarballFinalize::Written);
}

async fn indexed_refs(config: &Config, integrity: &str) -> Vec<Vec<u8>> {
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone());
    let integrity = integrity.parse().unwrap();
    let path = integrity_addressed_tarball_path(&integrity).unwrap();
    let digest = path.strip_prefix("-/tarballs/sha512/").unwrap();
    storage.read_hosted_revision_refs(digest).await.unwrap()
}

fn one_indexed() -> RevisionBackfillReport {
    RevisionBackfillReport {
        stores_scanned: 1,
        packages_scanned: 1,
        versions_scanned: 1,
        indexed: 1,
        already_indexed: 0,
        skipped: 0,
        invalid: 0,
    }
}

#[tokio::test]
async fn dry_run_verifies_without_writing_and_backfill_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let config = config_in(&tmp);
    let tarball = b"legacy hosted tarball";
    let integrity = sri(tarball, Algorithm::Sha512);
    seed_version(
        &config,
        "legacy",
        "1.0.0",
        json!({ "integrity": integrity, "tarball": "legacy-1.0.0.tgz" }),
        tarball,
    )
    .await;

    assert_eq!(backfill_hosted_revision_refs(&config, true).await.unwrap(), one_indexed());
    assert!(indexed_refs(&config, &integrity).await.is_empty());
    assert_eq!(backfill_hosted_revision_refs(&config, false).await.unwrap(), one_indexed());
    assert_eq!(indexed_refs(&config, &integrity).await.len(), 1);

    let mut rerun = one_indexed();
    rerun.indexed = 0;
    rerun.already_indexed = 1;
    assert_eq!(backfill_hosted_revision_refs(&config, false).await.unwrap(), rerun);
}

#[tokio::test]
async fn backfill_uses_revision_zero_after_a_replacement_is_selected() {
    let tmp = TempDir::new().unwrap();
    let config = config_in(&tmp);
    let original_tarball = b"original hosted tarball";
    let replacement_tarball = b"replacement tarball";
    let original = sri(original_tarball, Algorithm::Sha512);
    let replacement = sri(replacement_tarball, Algorithm::Sha512);
    seed_version(
        &config,
        "patched",
        "1.0.0",
        json!({
            "integrity": replacement,
            "revision": 1,
            "tarball": "-/tarballs/sha512/replacement",
            "revisions": [
                { "revision": 0, "integrity": original, "manifest": {} },
                { "revision": 1, "integrity": replacement, "manifest": {} },
            ],
        }),
        original_tarball,
    )
    .await;

    assert_eq!(backfill_hosted_revision_refs(&config, false).await.unwrap(), one_indexed());
    assert_eq!(indexed_refs(&config, &original).await.len(), 1);
    assert!(indexed_refs(&config, &replacement).await.is_empty());
}

#[tokio::test]
async fn backfill_rejects_mismatched_bytes_and_skips_non_sha512_metadata() {
    let tmp = TempDir::new().unwrap();
    let config = config_in(&tmp);
    let expected = b"expected";
    let integrity = sri(expected, Algorithm::Sha512);
    seed_version(&config, "tampered", "1.0.0", json!({ "integrity": integrity }), b"different")
        .await;

    let report = backfill_hosted_revision_refs(&config, false).await.unwrap();
    assert_eq!(report.invalid, 1);
    assert_eq!(report.indexed, 0);
    assert!(indexed_refs(&config, &integrity).await.is_empty());

    let legacy_tmp = TempDir::new().unwrap();
    let legacy_config = config_in(&legacy_tmp);
    let tarball = b"sha1 only";
    seed_version(
        &legacy_config,
        "sha1-only",
        "1.0.0",
        json!({ "integrity": sri(tarball, Algorithm::Sha1) }),
        tarball,
    )
    .await;
    let report = backfill_hosted_revision_refs(&legacy_config, false).await.unwrap();
    assert_eq!(report.skipped, 1);
    assert_eq!(report.invalid, 0);
}

#[tokio::test]
async fn backfill_supports_the_s3_hosted_store() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_in(&tmp);
    config.hosted_store =
        HostedStoreConfig::S3 { store: Arc::new(InMemory::new()), prefix: "tenant/".to_string() };
    let tarball = b"legacy s3 tarball";
    let integrity = sri(tarball, Algorithm::Sha512);
    seed_version(&config, "legacy-s3", "1.0.0", json!({ "integrity": integrity }), tarball).await;

    assert_eq!(backfill_hosted_revision_refs(&config, false).await.unwrap(), one_indexed());
    assert_eq!(indexed_refs(&config, &integrity).await.len(), 1);
}
