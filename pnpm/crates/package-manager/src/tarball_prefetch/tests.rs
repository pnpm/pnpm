use super::{PendingPrefetch, without_store_hits};
use pacquet_store_dir::{CafsFileInfo, PackageFilesIndex, StoreIndex, store_index_key};
use std::collections::HashMap;
use tempfile::tempdir;

fn sample_index() -> PackageFilesIndex {
    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo {
            checked_at: Some(1_700_000_000_000),
            digest: "abc".to_string(),
            mode: 0o644,
            size: 123,
        },
    );
    PackageFilesIndex {
        manifest: None,
        requires_build: Some(false),
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    }
}

fn pending(package_id: &str, integrity: &str) -> PendingPrefetch {
    PendingPrefetch {
        store_key: store_index_key(integrity, package_id),
        package_id: package_id.to_string(),
        package_url: format!("https://registry.example.com/{package_id}.tgz"),
        integrity: integrity.to_string(),
    }
}

#[tokio::test]
async fn without_store_hits_drops_entries_with_an_index_row() {
    let store = tempdir().unwrap();
    let warm = pending("@foo/warm@1.0.0", "sha512-aGVsbG8=");
    let cold = pending("@foo/cold@1.0.0", "sha512-d29ybGQ=");
    {
        let idx = StoreIndex::open(store.path()).unwrap();
        idx.set(&warm.store_key, &sample_index()).unwrap();
    }
    let index = StoreIndex::open_readonly(store.path())
        .map(|idx| std::sync::Arc::new(std::sync::Mutex::new(idx)))
        .ok();
    assert!(index.is_some(), "readonly index should open after a write");

    let remaining = without_store_hits(index, vec![warm, cold]).await;

    let remaining_ids: Vec<&str> =
        remaining.iter().map(|entry| entry.package_id.as_str()).collect();
    assert_eq!(remaining_ids, ["@foo/cold@1.0.0"]);
}

#[tokio::test]
async fn without_store_hits_keeps_everything_when_no_index_is_readable() {
    let warm = pending("@foo/warm@1.0.0", "sha512-aGVsbG8=");
    let cold = pending("@foo/cold@1.0.0", "sha512-d29ybGQ=");

    let remaining = without_store_hits(None, vec![warm, cold]).await;

    assert_eq!(remaining.len(), 2);
}
