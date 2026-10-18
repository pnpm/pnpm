use super::{HostedStore, upgrade};
use object_store::{
    ObjectStore, ObjectStoreExt as _, PutPayload, memory::InMemory, path::Path as ObjectPath,
};
use pnpr_config::HostedStoreConfig;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_storage::{HostedLayout, Storage};
use std::{collections::BTreeSet, path::Path, sync::Arc};
use tempfile::TempDir;

const LEGACY: [(&str, &[u8]); 9] = [
    ("left-pad/package.json", b"{\"name\":\"left-pad\"}"),
    ("left-pad/left-pad-1.0.0.tgz", b"tarball"),
    ("@scope/pkg/package.json", b"{\"name\":\"@scope/pkg\"}"),
    ("@scope/pkg/pkg-1.0.0.tgz", b"scoped"),
    (
        ".tombstones/v0/@scope/gone.json",
        b"{\"package\":\"@scope/gone\",\"unpublished_at\":\"2026-01-01T00:00:00Z\"}",
    ),
    ("acme/tool/package.json", b"{\"name\":\"tool\"}"),
    ("acme/tool/tool-1.0.0.tgz", b"org tarball"),
    (".staged/abc.json", b"{}"),
    ("stray/file", b"?"),
];

fn npm(name: &str) -> CanonicalPackageName {
    CanonicalPackageName::parse(name, Ecosystem::Npm).unwrap()
}

fn orgs() -> BTreeSet<String> {
    BTreeSet::from(["acme".to_string()])
}

async fn write_legacy(root: &Path) {
    for (path, bytes) in LEGACY {
        let path = root.join(path);
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(path, bytes).await.unwrap();
    }
}

/// What a converted store serves, in both namespaces.
async fn assert_converted(storage: &Storage) {
    assert_eq!(storage.hosted_layout().await.unwrap(), HostedLayout::Current);
    assert_eq!(storage.hosted_package_names().await.unwrap(), ["@scope/pkg", "left-pad"]);
    assert!(
        storage
            .open_hosted_blob(&npm("left-pad"), "left-pad-1.0.0.tgz")
            .await
            .unwrap()
            .is_some(),
    );
    assert!(
        storage
            .read_tombstone(&npm("@scope/gone"))
            .await
            .unwrap()
            .is_some(),
    );
    let org = storage.for_hosted("acme");
    assert_eq!(org.hosted_package_names().await.unwrap(), ["tool"]);
    assert!(
        org.open_hosted_blob(&npm("tool"), "tool-1.0.0.tgz").await
            .unwrap()
            .is_some(),
    );
    storage.ensure_hosted_layout().await.unwrap();
}

#[tokio::test]
async fn a_local_store_is_converted_in_place() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("storage");
    write_legacy(&root).await;
    tokio::fs::create_dir_all(root.join(".pnpr-journal")).await.unwrap();
    let storage =
        Storage::new(&HostedStoreConfig::Fs, root.clone(), tmp.path().join("cache")).unwrap();
    let store = HostedStore::Local(root.clone());

    let dry = upgrade(&storage, &store, &orgs(), true).await.unwrap();
    assert_eq!(dry.counts.get("documents"), Some(&3));
    assert_eq!(dry.orphans, ["stray/file"]);
    assert!(root.join("left-pad/package.json").exists());

    upgrade(&storage, &store, &orgs(), false).await.unwrap();
    assert_converted(&storage).await;
    assert!(!root.join("left-pad").exists());
    assert!(!root.join("acme").exists());
    assert!(root.join(".staged/abc.json").exists());
    assert!(
        root.join(".pnpr-journal").is_dir(),
        "an empty directory the upgrade did not touch stays",
    );
    assert!(root.join("stray/file").exists());
    assert!(upgrade(&storage, &store, &orgs(), false).await.unwrap().already_current);
}

#[tokio::test]
async fn an_interrupted_local_conversion_resumes() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("storage");
    write_legacy(&root).await;
    let moved = root.join("default/packages/left-pad/blobs/left-pad-1.0.0.tgz");
    tokio::fs::create_dir_all(moved.parent().unwrap()).await.unwrap();
    tokio::fs::rename(root.join("left-pad/left-pad-1.0.0.tgz"), &moved).await.unwrap();
    let storage =
        Storage::new(&HostedStoreConfig::Fs, root.clone(), tmp.path().join("cache")).unwrap();

    upgrade(&storage, &HostedStore::Local(root), &orgs(), false).await.unwrap();
    assert_converted(&storage).await;
}

#[tokio::test]
async fn an_object_store_is_converted_with_its_prefix() {
    let tmp = TempDir::new().unwrap();
    let objects: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    for (path, bytes) in LEGACY {
        objects
            .put(&ObjectPath::from(format!("packages/{path}")), PutPayload::from_static(bytes))
            .await
            .unwrap();
    }
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&objects), prefix: "packages".into() };
    let storage =
        Storage::new(&config, tmp.path().join("storage"), tmp.path().join("cache")).unwrap();
    let store = HostedStore::Objects { store: objects, prefix: "packages/".into() };

    upgrade(&storage, &store, &orgs(), false).await.unwrap();
    assert_converted(&storage).await;
}
