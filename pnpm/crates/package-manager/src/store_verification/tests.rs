use super::StoreVerification;
use pnpm_store_dir::{
    CafsFileInfo, PackageFilesIndex, SharedVerifiedFilesCache, StoreDir, StoreIndex,
    store_index_key,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tempfile::{TempDir, tempdir};

/// A row whose one file is in the store, and a row whose file is not. The
/// directory lives as long as the returned handle.
fn store_with_two_rows() -> (TempDir, &'static StoreDir, String, String) {
    let tmp = tempdir().unwrap();
    let store_dir: &'static StoreDir = Box::leak(Box::new(StoreDir::new(tmp.path())));
    let (_, hash) = store_dir.write_cas_file(b"hello", false).unwrap();
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let row = |digest: String| PackageFilesIndex {
        manifest: None,
        requires_build: Some(false),
        requires_prepare: None,
        algo: "sha512".to_string(),
        files: HashMap::from([(
            "index.js".to_string(),
            CafsFileInfo { checked_at: Some(now_ms), digest, mode: 0o644, size: 5 },
        )]),
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    let present = store_index_key("sha512-aGVsbG8=", "present@1.0.0");
    let gone = store_index_key("sha512-d29ybGQ=", "gone@1.0.0");
    let index = StoreIndex::open_in(store_dir).unwrap();
    index
        .set(&present, &row(format!("{hash:x}")))
        .unwrap();
    index
        .set(&gone, &row("ab".repeat(64)))
        .unwrap();
    (tmp, store_dir, present, gone)
}

#[tokio::test]
async fn finish_hands_over_the_rows_whose_files_are_in_place() {
    let (_tmp, store_dir, present, gone) = store_with_two_rows();
    let index = Arc::new(Mutex::new(StoreIndex::open_readonly_in(store_dir).unwrap()));
    let files = SharedVerifiedFilesCache::default();

    let verification = StoreVerification::default();
    verification.start(vec![present.clone(), gone.clone()], &index, store_dir, true, &files);
    let verified = verification.finish().await.expect("a started verification finishes");

    assert!(verified.rows.cas_paths.contains_key(&present));
    assert!(
        !verified.rows.cas_paths.contains_key(&gone),
        "a row whose file is missing is left to the fetch",
    );
    assert_eq!(verified.files.len(), 1, "the file checked is in the cache the fetch inherits");
}

#[tokio::test]
async fn finish_without_a_start_is_no_verification() {
    assert!(StoreVerification::default().finish().await.is_none());
}

#[tokio::test]
async fn dropping_the_verification_aborts_its_task() {
    let (_tmp, store_dir, present, gone) = store_with_two_rows();
    let index = Arc::new(Mutex::new(StoreIndex::open_readonly_in(store_dir).unwrap()));
    let files = SharedVerifiedFilesCache::default();

    let verification = StoreVerification::default();
    verification.start(vec![present, gone], &index, store_dir, true, &files);
    let task = verification.started
        .lock()
        .unwrap()
        .as_ref()
        .expect("started")
        .task
        .abort_handle();
    drop(verification);

    for _ in 0..100 {
        if task.is_finished() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the verification task keeps running after its owner is dropped");
}
