use super::{
    JournaledPublish, MANIFEST_FILE, Manifest, cleanup_conflicted_tmp_paths,
    drop_conflicted_versions, roll_forward, sync_dir,
};
use crate::{
    config::HostedStoreConfig,
    package_name::PackageName,
    storage::{Storage, TarballFinalize},
};
use object_store::{ObjectStore, memory::InMemory};
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
            // A version with no resolvable tarball basename is kept as-is.
            "3.0.0": { "dist": {} },
        }
    });
    let conflicted: HashSet<&str> = std::iter::once("pkg-1.0.0.tgz").collect();

    drop_conflicted_versions(&mut journaled, &conflicted);

    let versions = journaled["versions"].as_object().unwrap();
    assert!(!versions.contains_key("1.0.0"));
    assert!(versions.contains_key("2.0.0"));
    assert!(versions.contains_key("3.0.0"));
}

#[test]
fn drop_conflicted_versions_tolerates_a_missing_versions_map() {
    let mut journaled = json!({ "name": "pkg" });
    let conflicted: HashSet<&str> = std::iter::once("pkg-1.0.0.tgz").collect();
    drop_conflicted_versions(&mut journaled, &conflicted);
    assert_eq!(journaled, json!({ "name": "pkg" }));
}

#[test]
fn drop_conflicted_versions_uses_shared_tarball_url_semantics() {
    let mut journaled = json!({
        "versions": {
            "1.0.0": { "dist": { "tarball": "http://host/pkg/-/pkg-1.0.0.tgz?sig=x" } },
            "2.0.0": { "dist": { "tarball": "http://host/pkg/-/pkg-2.0.0.tgz#fragment" } },
            "3.0.0": { "dist": { "tarball": "http://host/pkg/-/" } },
        }
    });
    let conflicted: HashSet<&str> = ["pkg-1.0.0.tgz", "pkg-2.0.0.tgz"].into_iter().collect();

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
    let conflicted: HashSet<&str> = std::iter::once("pkg-1.0.0.tgz").collect();

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
async fn roll_forward_preserves_tarball_conflict_across_a_later_package_failure() {
    let tmp = tempdir().unwrap();
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let storage = Storage::new(
        &HostedStoreConfig::S3 { store: object_store, prefix: String::new() },
        tmp.path().join("hosted"),
        tmp.path().join("cache"),
    );
    let conflicted_name = PackageName::parse("conflicted-pkg").unwrap();
    let later_name = PackageName::parse("later-pkg").unwrap();
    let filename = "conflicted-pkg-1.0.0.tgz";

    let winner = storage.reserve_hosted_tarball(&conflicted_name, filename).await.unwrap();
    fs::write(&winner.tmp_path, b"winner").await.unwrap();
    assert_eq!(storage.finalize_tarball_slot(winner).await.unwrap(), TarballFinalize::Written,);

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
                    "tarball": "http://host/conflicted-pkg/-/conflicted-pkg-1.0.0.tgz",
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
        },
        JournaledPublish { name: &later_name, org: None, packument: b"not-json", slots: &[] },
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

    roll_forward(&storage, &txn_dir).await.unwrap();

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
