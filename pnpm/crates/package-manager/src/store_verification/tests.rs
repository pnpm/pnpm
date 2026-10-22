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

/// `present` rows whose one file each is in the store, and a row whose file
/// is not. The directory lives as long as the returned handle.
fn store_with_rows(present: usize) -> (TempDir, &'static StoreDir, Vec<String>, String) {
    let tmp = tempdir().unwrap();
    let store_dir: &'static StoreDir = Box::leak(Box::new(StoreDir::new(tmp.path())));
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let row = |digest: String, size: u64| PackageFilesIndex {
        manifest: None,
        requires_build: Some(false),
        requires_prepare: None,
        algo: "sha512".to_string(),
        files: HashMap::from([(
            "index.js".to_string(),
            CafsFileInfo { checked_at: Some(now_ms), digest, mode: 0o644, size },
        )]),
        side_effects: None,
        remote_side_effects_quarantine: None,
        tar_integrity: None,
    };
    let index = StoreIndex::open_in(store_dir).unwrap();
    let present_keys = (0..present)
        .map(|n| {
            let content = format!("hello-{n}");
            let (_, hash) = store_dir.write_cas_file(content.as_bytes(), false).unwrap();
            let key = store_index_key(&format!("sha512-{n:04}"), &format!("present-{n}@1.0.0"));
            index
                .set(&key, &row(format!("{hash:x}"), content.len() as u64))
                .unwrap();
            key
        })
        .collect();
    let gone = store_index_key("sha512-d29ybGQ=", "gone@1.0.0");
    index
        .set(&gone, &row("ab".repeat(64), 5))
        .unwrap();
    (tmp, store_dir, present_keys, gone)
}

/// Yield until `done`, or fail after a bounded number of turns.
async fn wait_until(what: &str, done: impl Fn() -> bool) {
    for _ in 0..10_000 {
        if done() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("{what}");
}

#[tokio::test]
async fn finish_hands_over_the_rows_whose_files_are_in_place() {
    let (_tmp, store_dir, present, gone) = store_with_rows(1);
    let index = Arc::new(Mutex::new(StoreIndex::open_readonly_in(store_dir).unwrap()));
    let files = SharedVerifiedFilesCache::default();

    let verification = StoreVerification::default();
    verification.start(vec![present[0].clone(), gone.clone()], &index, store_dir, true, &files);
    let verified = verification.finish().await.expect("a started verification finishes");

    assert!(verified.keys.contains(&present[0]));
    assert!(!verified.keys.contains(&gone), "a row whose file is missing is left to the fetch");
    assert_eq!(verified.files.len(), 1, "the file checked is in the cache the fetch inherits");
}

#[tokio::test]
async fn every_batch_is_verified() {
    let (_tmp, store_dir, present, gone) = store_with_rows(7);
    let index = Arc::new(Mutex::new(StoreIndex::open_readonly_in(store_dir).unwrap()));
    let files = SharedVerifiedFilesCache::default();

    let verification = StoreVerification::default();
    let mut keys = present.clone();
    keys.push(gone);
    verification.start_in_batches(keys, &index, store_dir, true, &files, 3);
    let verified = verification.finish().await.expect("a started verification finishes");

    assert_eq!(verified.keys.len(), 7);
    assert!(
        present
            .iter()
            .all(|key| verified.keys.contains(key)),
    );
}

#[tokio::test]
async fn finish_without_a_start_is_no_verification() {
    assert!(StoreVerification::default().finish().await.is_none());
}

#[tokio::test]
async fn dropping_the_verification_stops_its_work_at_the_next_batch() {
    let (_tmp, store_dir, present, _gone) = store_with_rows(12);
    let index = Arc::new(Mutex::new(StoreIndex::open_readonly_in(store_dir).unwrap()));
    let files = SharedVerifiedFilesCache::default();

    let verification = StoreVerification::default();
    verification.start_in_batches(present, &index, store_dir, true, &files, 1);
    let task = verification.started
        .lock()
        .unwrap()
        .as_ref()
        .expect("started")
        .task
        .abort_handle();
    wait_until("the verification never checks a file", || !files.is_empty()).await;
    drop(verification);

    wait_until("the verification task keeps running after its owner is dropped", || {
        task.is_finished()
    })
    .await;
    assert!(files.len() < 12, "no batch starts once the verification is dropped");
}

#[tokio::test]
async fn a_second_start_stops_the_first() {
    let (_tmp, store_dir, present, _gone) = store_with_rows(2);
    let index = Arc::new(Mutex::new(StoreIndex::open_readonly_in(store_dir).unwrap()));
    let files = SharedVerifiedFilesCache::default();

    let verification = StoreVerification::default();
    verification.start_in_batches(present.clone(), &index, store_dir, true, &files, 1);
    let first = verification.started
        .lock()
        .unwrap()
        .as_ref()
        .expect("started")
        .task
        .abort_handle();
    verification.start(present, &index, store_dir, true, &files);

    wait_until("the replaced verification keeps running", || first.is_finished()).await;
    assert_eq!(
        verification
            .finish()
            .await
            .expect("the second finishes")
            .keys
            .len(),
        2,
    );
}
