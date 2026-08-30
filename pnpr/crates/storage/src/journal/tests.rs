use super::{
    JournaledPublish, JournaledRevisionRef, MANIFEST_FILE, Manifest, cleanup_conflicted_tmp_paths,
    drop_conflicted_versions, revision_ref_owner, roll_forward, sync_dir,
};
use crate::{HostedRevisionRefWrite, Storage, TarballFinalize};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use object_store::{ObjectStore, memory::InMemory};
use pnpr_config::HostedStoreConfig;
use pnpr_package_name::PackageName;
use serde_json::json;
use std::{collections::HashSet, sync::Arc};
use tempfile::tempdir;
use tokio::fs;

#[test]
fn drop_conflicted_versions_removes_only_the_lost_versions() {
    let mut journaled = json!({
        "versions": {
            "1.0.0": { "dist": { "tarball": "http://host/pkg/-/pkg-1.0.0.tgz" } },
            "2.0.0": { "dist": { "tarball": "http://host/pkg/-/pkg-2.0.0.tgz" } },
            // A version outside the conflict set is kept as-is.
            "3.0.0": { "dist": {} },
        }
    });
    let conflicted: HashSet<String> = std::iter::once("1.0.0".to_string()).collect();

    drop_conflicted_versions(&mut journaled, &conflicted);

    let versions = journaled["versions"].as_object().unwrap();
    assert!(!versions.contains_key("1.0.0"));
    assert!(versions.contains_key("2.0.0"));
    assert!(versions.contains_key("3.0.0"));
}

#[test]
fn drop_conflicted_versions_tolerates_a_missing_versions_map() {
    let mut journaled = json!({ "name": "pkg" });
    let conflicted: HashSet<String> = std::iter::once("1.0.0".to_string()).collect();
    drop_conflicted_versions(&mut journaled, &conflicted);
    assert_eq!(journaled, json!({ "name": "pkg" }));
}

#[test]
fn drop_conflicted_versions_uses_the_canonical_attachment_version() {
    let mut journaled = json!({
        "versions": {
            "1.0.0": { "dist": { "tarball": "http://host/pkg/-/publisher-chosen-name.tgz" } },
            "2.0.0": { "dist": { "tarball": "http://host/pkg/-/another-name.tgz" } },
            "3.0.0": { "dist": { "tarball": "http://host/pkg/-/" } },
        }
    });
    let conflicted: HashSet<String> =
        ["1.0.0".to_string(), "2.0.0".to_string()].into_iter().collect();

    drop_conflicted_versions(&mut journaled, &conflicted);

    let versions = journaled["versions"].as_object().unwrap();
    let remaining_versions: Vec<_> = versions.keys().map(String::as_str).collect();
    assert_eq!(remaining_versions, vec!["3.0.0"]);
}

#[test]
fn drop_conflicted_versions_removes_references_to_lost_versions() {
    let mut journaled = json!({
        "versions": {
            "1.0.0": { "dist": { "tarball": "http://host/pkg/-/pkg-1.0.0.tgz" } },
            "2.0.0": { "dist": { "tarball": "http://host/pkg/-/pkg-2.0.0.tgz" } },
        },
        "dist-tags": {
            "latest": "1.0.0",
            "next": "2.0.0",
            "opaque": 42,
        },
        "time": {
            "1.0.0": "2026-07-01T00:00:00.000Z",
            "2.0.0": "2026-07-02T00:00:00.000Z",
            "modified": "2026-07-03T00:00:00.000Z",
        },
    });
    let conflicted: HashSet<String> = std::iter::once("1.0.0".to_string()).collect();

    drop_conflicted_versions(&mut journaled, &conflicted);

    assert_eq!(journaled["dist-tags"], json!({ "next": "2.0.0", "opaque": 42 }));
    assert_eq!(
        journaled["time"],
        json!({
            "2.0.0": "2026-07-02T00:00:00.000Z",
            "modified": "2026-07-03T00:00:00.000Z",
        }),
    );
}

#[tokio::test]
async fn cleanup_keeps_conflicted_tmp_when_journal_removal_is_not_durable() {
    let tmp = tempdir().unwrap();
    let tmp_path = tmp.path().join("conflicted.tmp");
    fs::write(&tmp_path, b"loser").await.unwrap();

    cleanup_conflicted_tmp_paths(&[tmp_path.as_path()], false).await;

    assert!(fs::try_exists(tmp_path).await.unwrap());
}

#[tokio::test]
async fn cleanup_removes_conflicted_tmp_when_journal_removal_is_durable() {
    let tmp = tempdir().unwrap();
    let tmp_path = tmp.path().join("conflicted.tmp");
    fs::write(&tmp_path, b"loser").await.unwrap();

    cleanup_conflicted_tmp_paths(&[tmp_path.as_path()], true).await;

    assert!(!fs::try_exists(tmp_path).await.unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn sync_dir_reports_success_for_a_directory() {
    let tmp = tempdir().unwrap();

    sync_dir(tmp.path()).await.unwrap();
}

#[tokio::test]
async fn roll_forward_persists_revision_references() {
    let tmp = tempdir().unwrap();
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let storage = Storage::new(
        &HostedStoreConfig::ObjectStore { store: object_store, prefix: String::new() },
        tmp.path().join("hosted"),
        tmp.path().join("cache"),
    )
    .unwrap();
    let name = PackageName::parse("pkg").unwrap();
    let packument = serde_json::to_vec(&json!({
        "name": "pkg",
        "versions": {},
    }))
    .unwrap();
    let digest = URL_SAFE_NO_PAD.encode([7_u8; 64]);
    let record = br#"{"package":"pkg","version":"1.0.0"}"#.to_vec();
    let revision_refs = [JournaledRevisionRef {
        filename: "pkg-1.0.0.tgz".to_string(),
        digest: digest.clone(),
        ref_id: "a".repeat(64),
        bytes: record.clone(),
    }];
    let entries = [JournaledPublish {
        name: &name,
        org: None,
        packument: &packument,
        slots: &[],
        revision_refs: &revision_refs,
    }];

    storage.publish_journal().seal(&entries).await.unwrap().roll_forward(&storage).await.unwrap();

    assert_eq!(storage.read_hosted_revision_refs(&digest).await.unwrap(), vec![record.clone()]);
    assert_eq!(
        storage
            .write_hosted_revision_ref(&digest, &"a".repeat(64), "later-owner", &record)
            .await
            .unwrap(),
        HostedRevisionRefWrite::Committed,
    );
}

#[tokio::test]
async fn roll_forward_drops_a_version_that_cannot_reserve_a_revision_reference() {
    let tmp = tempdir().unwrap();
    let storage =
        Storage::new(&HostedStoreConfig::Fs, tmp.path().join("hosted"), tmp.path().join("cache"))
            .unwrap();
    let digest = URL_SAFE_NO_PAD.encode([7_u8; 64]);
    for index in 0..crate::MAX_HOSTED_REVISION_REFS {
        storage
            .write_hosted_revision_ref(&digest, &format!("{index:064x}"), "existing-owner", b"{}")
            .await
            .unwrap();
    }
    let name = PackageName::parse("pkg").unwrap();
    let packument = serde_json::to_vec(&json!({
        "name": "pkg",
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "dist": { "tarball": "http://host/pkg/-/publisher-chosen-name.tgz" },
            },
        },
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2026-07-01T00:00:00.000Z" },
    }))
    .unwrap();
    let revision_refs = [JournaledRevisionRef {
        filename: "pkg-1.0.0.tgz".to_string(),
        digest: digest.clone(),
        ref_id: "f".repeat(64),
        bytes: br#"{"package":"pkg","version":"1.0.0"}"#.to_vec(),
    }];
    let entries = [JournaledPublish {
        name: &name,
        org: None,
        packument: &packument,
        slots: &[],
        revision_refs: &revision_refs,
    }];

    storage.publish_journal().seal(&entries).await.unwrap().roll_forward(&storage).await.unwrap();

    let hosted = storage.read_hosted_packument(&name).await.unwrap().unwrap();
    let hosted: serde_json::Value = serde_json::from_slice(&hosted).unwrap();
    assert_eq!(hosted["versions"], json!({}));
    assert_eq!(hosted["dist-tags"], json!({}));
    assert_eq!(hosted["time"].get("1.0.0"), None);
    assert_eq!(
        storage.read_hosted_revision_refs(&digest).await.unwrap().len(),
        crate::MAX_HOSTED_REVISION_REFS,
    );
}

#[tokio::test]
async fn roll_forward_only_removes_transaction_owned_references_for_a_dropped_version() {
    let tmp = tempdir().unwrap();
    let storage =
        Storage::new(&HostedStoreConfig::Fs, tmp.path().join("hosted"), tmp.path().join("cache"))
            .unwrap();
    let transaction_owned_digest = URL_SAFE_NO_PAD.encode([5_u8; 64]);
    let previously_owned_digest = URL_SAFE_NO_PAD.encode([6_u8; 64]);
    let full_digest = URL_SAFE_NO_PAD.encode([7_u8; 64]);
    for index in 0..crate::MAX_HOSTED_REVISION_REFS {
        storage
            .write_hosted_revision_ref(
                &full_digest,
                &format!("{index:064x}"),
                "existing-owner",
                b"{}",
            )
            .await
            .unwrap();
    }
    let name = PackageName::parse("pkg").unwrap();
    let packument = serde_json::to_vec(&json!({
        "name": "pkg",
        "versions": { "1.0.0": { "version": "1.0.0" } },
    }))
    .unwrap();
    let ref_id = "f".repeat(64);
    let record = br#"{"package":"pkg","version":"1.0.0"}"#.to_vec();
    let revision_refs = [
        JournaledRevisionRef {
            filename: "pkg-1.0.0.tgz".to_string(),
            digest: transaction_owned_digest.clone(),
            ref_id: ref_id.clone(),
            bytes: record.clone(),
        },
        JournaledRevisionRef {
            filename: "pkg-1.0.0.tgz".to_string(),
            digest: previously_owned_digest.clone(),
            ref_id: ref_id.clone(),
            bytes: record.clone(),
        },
        JournaledRevisionRef {
            filename: "pkg-1.0.0.tgz".to_string(),
            digest: full_digest,
            ref_id: ref_id.clone(),
            bytes: record.clone(),
        },
    ];
    let entries = [JournaledPublish {
        name: &name,
        org: None,
        packument: &packument,
        slots: &[],
        revision_refs: &revision_refs,
    }];

    let txn = storage.publish_journal().seal(&entries).await.unwrap();
    let revision_ref_owner = txn.revision_ref_owner().to_string();
    storage
        .write_hosted_revision_ref(&transaction_owned_digest, &ref_id, &revision_ref_owner, &record)
        .await
        .unwrap();
    storage
        .write_hosted_revision_ref(&previously_owned_digest, &ref_id, "previous-owner", &record)
        .await
        .unwrap();
    storage
        .commit_hosted_revision_ref(&previously_owned_digest, &ref_id, "previous-owner")
        .await
        .unwrap();
    txn.roll_forward(&storage).await.unwrap();

    assert_eq!(
        storage.read_hosted_revision_refs(&transaction_owned_digest).await.unwrap(),
        Vec::<Vec<u8>>::new(),
    );
    assert_eq!(
        storage.read_hosted_revision_refs(&previously_owned_digest).await.unwrap(),
        vec![record],
    );
    let hosted = storage.read_hosted_packument(&name).await.unwrap().unwrap();
    let hosted: serde_json::Value = serde_json::from_slice(&hosted).unwrap();
    assert_eq!(hosted["versions"], json!({}));
}

#[tokio::test]
async fn roll_forward_preserves_tarball_conflict_across_a_later_package_failure() {
    let tmp = tempdir().unwrap();
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let storage = Storage::new(
        &HostedStoreConfig::ObjectStore { store: object_store, prefix: String::new() },
        tmp.path().join("hosted"),
        tmp.path().join("cache"),
    )
    .unwrap();
    let conflicted_name = PackageName::parse("conflicted-pkg").unwrap();
    let later_name = PackageName::parse("later-pkg").unwrap();
    let filename = "conflicted-pkg-1.0.0.tgz";

    let winner = storage.reserve_hosted_tarball(&conflicted_name, filename).await.unwrap();
    fs::write(&winner.tmp_path, b"winner").await.unwrap();
    assert_eq!(storage.finalize_tarball_slot(winner).await.unwrap(), TarballFinalize::Written);

    let loser = storage.reserve_hosted_tarball(&conflicted_name, filename).await.unwrap();
    fs::write(&loser.tmp_path, b"loser").await.unwrap();
    let loser_tmp_path = loser.tmp_path.clone();
    let conflicted_slots = [loser];
    let conflicted_packument = serde_json::to_vec(&json!({
        "name": "conflicted-pkg",
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://host/conflicted-pkg/-/publisher-chosen-name.tgz",
                    "integrity": "loser",
                },
            },
        },
        "dist-tags": { "latest": "1.0.0" },
        "time": {
            "1.0.0": "2026-07-01T00:00:00.000Z",
            "modified": "2026-07-01T00:00:00.000Z",
        },
    }))
    .unwrap();
    let entries = [
        JournaledPublish {
            name: &conflicted_name,
            org: None,
            packument: &conflicted_packument,
            slots: &conflicted_slots,
            revision_refs: &[],
        },
        JournaledPublish {
            name: &later_name,
            org: None,
            packument: b"not-json",
            slots: &[],
            revision_refs: &[],
        },
    ];
    let txn = storage.publish_journal().seal(&entries).await.unwrap();
    let txn_dir = txn.dir.clone();

    drop(txn.roll_forward(&storage).await.unwrap_err());
    assert!(
        fs::try_exists(&loser_tmp_path).await.unwrap(),
        "충돌한 임시 tarball은 트랜잭션 재시도를 위해 남아 있어야 합니다",
    );
    assert!(
        fs::try_exists(&txn_dir).await.unwrap(),
        "뒤 패키지가 실패하면 journal이 재시도를 위해 남아 있어야 합니다",
    );

    let manifest: Manifest =
        serde_json::from_slice(&fs::read(txn_dir.join(MANIFEST_FILE)).await.unwrap()).unwrap();
    let later_packument = json!({
        "name": "later-pkg",
        "versions": {
            "2.0.0": { "version": "2.0.0" },
        },
    });
    let later =
        manifest.packages.iter().find(|package| package.name == later_name.as_str()).unwrap();
    fs::write(txn_dir.join(&later.packument_file), serde_json::to_vec(&later_packument).unwrap())
        .await
        .unwrap();

    roll_forward(&storage, &txn_dir, revision_ref_owner(&txn_dir).unwrap()).await.unwrap();

    let conflicted_hosted = storage.read_hosted_packument(&conflicted_name).await.unwrap().unwrap();
    let conflicted_hosted: serde_json::Value = serde_json::from_slice(&conflicted_hosted).unwrap();
    assert_eq!(conflicted_hosted["versions"], json!({}));
    assert_eq!(conflicted_hosted["dist-tags"], json!({}));
    assert_eq!(conflicted_hosted["time"].get("1.0.0"), None);
    let later_hosted = storage.read_hosted_packument(&later_name).await.unwrap().unwrap();
    let later_hosted: serde_json::Value = serde_json::from_slice(&later_hosted).unwrap();
    assert_eq!(later_hosted["versions"]["2.0.0"]["version"], "2.0.0");
    #[cfg(unix)]
    assert!(
        !fs::try_exists(&loser_tmp_path).await.unwrap(),
        "내구성 있는 journal 삭제 뒤에는 충돌한 임시 tarball을 정리해야 합니다",
    );
    assert!(
        !fs::try_exists(&txn_dir).await.unwrap(),
        "완료된 트랜잭션은 journal을 제거해야 합니다",
    );
}
