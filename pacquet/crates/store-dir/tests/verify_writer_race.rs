//! Reproducer for the writer × verifier race that surfaces in CI as
//! `failed to import "<store>/v11/files/…": No such file or directory (os error 2)`.
//!
//! Scenario:
//!
//! 1. A writer thread is inside [`pacquet_fs::ensure_file`] for path
//!    `F`, holding the per-path `cas_write_lock`. Between
//!    `O_CREAT|O_EXCL` and `write_all` completion, `F` exists on
//!    disk with a partial body.
//! 2. A verifier thread runs [`pacquet_store_dir::check_pkg_files_integrity`]
//!    for an index row that references `F` (different package, same
//!    content hash — common for shared boilerplate like `LICENSE`
//!    files across sibling packages).
//! 3. Verifier `stat`s `F` (no lock), sees a size that doesn't
//!    match the index, and calls `remove_stale_cafs_entry(F)` to
//!    scrub the "torn" blob.
//! 4. The writer's `write_all` continues — but `F`'s dirent is now
//!    gone. The kernel keeps the inode alive while the fd is open;
//!    once the writer drops the fd, the inode is freed. The
//!    writer's `ensure_file` returns `Ok(())` and the install
//!    proceeds with `cas_paths` pointing at a path that no longer
//!    exists.
//! 5. Downstream `link_file` hits `ENOENT` and the install fails.
//!
//! Option C fix: extend `cas_write_lock` to `verify_file` so the
//! verifier waits for the writer to finish before deciding whether
//! the file is stale. With the fix, the verifier always observes
//! the writer's final state — either fully-written and valid (skip
//! the delete) or genuinely stale (delete safely).
//!
//! The race is timing-dependent in production because the
//! writer's `write_all` is fast (milliseconds for a small file).
//! To make the reproducer deterministic, the test holds
//! `cas_write_lock` from the main thread for the entire critical
//! window — this is exactly what an in-flight `ensure_file` would
//! do, just on a wall-clock the test controls. The verifier runs
//! in the background while the lock is held; without the fix it
//! unlinks the file unconditionally, with the fix it blocks on
//! the lock until the simulated writer releases it.

use std::{
    fs,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use pacquet_store_dir::{
    CafsFileInfo, PackageFilesIndex, StoreDir, VerifiedFilesCache, check_pkg_files_integrity,
};
use sha2::{Digest, Sha512};
use tempfile::tempdir;

const CONTENT_SIZE: usize = 64 * 1024;

fn sha512_hex(content: &[u8]) -> String {
    let digest = Sha512::digest(content);
    format!("{digest:x}")
}

/// Place a `CafsFileInfo` row that the verifier will check against,
/// with `checked_at` pinned to the Unix epoch so the file's current
/// mtime is always > 100 ms past the recorded check time — the
/// condition that makes `verify_file` enter its destructive branch.
fn make_index(filename: &str, content: &[u8]) -> PackageFilesIndex {
    let mut files = std::collections::HashMap::new();
    files.insert(
        filename.to_string(),
        CafsFileInfo {
            digest: sha512_hex(content),
            mode: 0o644,
            size: content.len() as u64,
            checked_at: Some(0),
        },
    );
    PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    }
}

/// Compute the CAS path the writer + verifier agree on for a given
/// content hash. Mirrors `StoreDir::cas_file_path_by_mode` for
/// `mode = 0o644` (no `-exec` suffix).
fn cas_path_for(store: &StoreDir, content: &[u8]) -> std::path::PathBuf {
    store
        .cas_file_path_by_mode(&sha512_hex(content), 0o644)
        .expect("sha512 hex is always a valid CAFS path")
}

/// The reproducer.
///
/// Pre-Option-C: the verifier deletes the file while the simulated
/// writer "holds the lock", because `verify_file` doesn't acquire the
/// lock at all.
///
/// Post-Option-C: the verifier acquires `cas_write_lock(path)`
/// before deciding whether to delete. While the test holds the
/// lock, the verifier blocks; we observe the file is still on disk.
/// Then the test releases the lock, the verifier finishes, and we
/// observe the file is still there (because the verifier now sees
/// the final committed state).
#[test]
fn verify_does_not_unlink_file_while_writer_holds_cas_lock() {
    let tmp = tempdir().expect("tempdir");
    let store = StoreDir::new(tmp.path().to_path_buf());

    let expected_content: Vec<u8> = (0..CONTENT_SIZE).map(|i| (i % 256) as u8).collect();
    let target = cas_path_for(&store, &expected_content);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).expect("create shard dir");
    }

    // Simulate "writer is mid-`write_all`": F exists on disk with a
    // partial prefix of the expected content. Size mismatch — exactly
    // the condition that drives `verify_file` into the
    // `remove_stale_cafs_entry` branch.
    fs::write(&target, &expected_content[..1024]).expect("pre-seed partial blob");

    // The main thread "becomes" the writer: acquire the per-path
    // `cas_write_lock` and hold it across the verifier's run. The
    // lock is the only primitive `ensure_file` uses to gate concurrent
    // writers; with Option C, `verify_file` acquires the same lock
    // before considering a delete.
    let lock = pacquet_fs::cas_write_lock(&target);
    let guard = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    // Use a channel to synchronize lock-release with the verifier so
    // we can assert the file's state at a known point.
    let (verifier_started_tx, verifier_started_rx) = mpsc::channel::<()>();
    let result_slot: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));

    let verify_store = StoreDir::new(tmp.path().to_path_buf());
    let verify_content = expected_content.clone();
    let result_slot_writer = Arc::clone(&result_slot);
    let target_for_verifier = target.clone();
    let verifier = thread::spawn(move || {
        verifier_started_tx.send(()).expect("send start");
        let pkg_index = make_index("LICENSE", &verify_content);
        let cache = VerifiedFilesCache::new();
        let result = check_pkg_files_integrity(&verify_store, pkg_index, &cache);
        // Record whether the file survived the verifier's run.
        *result_slot_writer.lock().expect("result mutex") =
            Some((target_for_verifier.exists(), result.passed).0);
    });

    // Wait until the verifier thread has actually started.
    verifier_started_rx.recv().expect("verifier started");

    // Sleep to give the verifier time to either:
    //   - (pre-fix) charge ahead, stat the partial file, unlink it
    //     before we can react, OR
    //   - (post-fix) block on `cas_write_lock` and make no progress.
    // 200 ms is far more than enough for either outcome on any
    // realistic runner.
    thread::sleep(Duration::from_millis(200));

    // Snapshot the on-disk state while we still hold the simulated
    // writer's lock. This is the assertion that distinguishes the
    // two implementations.
    let file_exists_under_lock = target.exists();

    // Simulate the writer "finishing" by replacing the partial blob
    // with the full content — what `ensure_file`'s `write_all` +
    // close would have produced. Drop the lock guard so the verifier
    // (if it's blocked) can wake up and observe the final state.
    fs::write(&target, &expected_content).expect("commit full content");
    drop(guard);

    verifier.join().expect("verifier thread should not panic");
    let file_exists_after_verify = target.exists();

    // The killer assertion. Without Option C the verifier doesn't
    // wait — it sees the partial file under the lock and unlinks it,
    // which means `file_exists_under_lock` is `false`. With Option
    // C, the verifier waits on the lock and never makes a decision
    // until we release it, so `file_exists_under_lock` is `true`.
    assert!(
        file_exists_under_lock,
        "Verifier unlinked the CAS file while a writer was still holding cas_write_lock. \
         This is the writer-vs-verifier race that surfaces as ENOENT inside link_file at \
         install time. Option C (extending cas_write_lock to verify_file) is what closes it.",
    );

    // Also assert the file is still present after the verifier finished:
    // with the fix, once the lock releases the verifier sees the full
    // content (we wrote it under the lock) and either skips the delete
    // (matching hash) or deletes safely with no writer racing it.
    // Without the fix this would already be false from the unlink above,
    // so the earlier assert catches it first; we keep this as a final
    // contract check.
    assert!(
        file_exists_after_verify,
        "After the simulated writer released its lock, the verifier should observe \
         the final (correct) content and not delete it",
    );

    drop(tmp);
}

/// Sanity check: the lock acquired by [`pacquet_fs::cas_write_lock`]
/// keys on the absolute path. Two callers asking for the same path
/// receive a reference to the same `Mutex` — the property the Option-C
/// fix relies on for the verifier to wait on the writer.
#[test]
fn cas_write_lock_returns_same_mutex_for_same_path() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("shared-blob");

    let lock_a = pacquet_fs::cas_write_lock(&path);
    let lock_b = pacquet_fs::cas_write_lock(&path);

    assert!(
        std::ptr::eq(lock_a, lock_b),
        "cas_write_lock must hand out the same Mutex<()> for the same path",
    );
}
