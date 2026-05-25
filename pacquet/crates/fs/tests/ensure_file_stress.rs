//! Cross-process stress tests for [`pacquet_fs::ensure_file`].
//!
//! Ports the three multi-process scenarios upstream pnpm covers in
//! [`store/cafs/test/writeBufferToCafs.test.ts`](https://github.com/pnpm/pnpm/blob/8695496f58/store/cafs/test/writeBufferToCafs.test.ts):
//!
//! 1. Concurrent writes of the same content from many processes all
//!    succeed and converge on a byte-identical CAS file.
//! 2. The same scenario after a previous (crashed) writer has left
//!    a corrupt blob at the target path — recovery via
//!    `verify_or_rewrite` + `write_atomic` rewrites the blob and
//!    every concurrent writer still returns success.
//! 3. The same scenario after a previous (crashed) writer has left
//!    a truncated prefix of the correct content — the size-mismatch
//!    fast path inside `verify_or_rewrite` kicks in and the
//!    overwrite-via-rename heals the store.
//!
//! Pacquet's [`cas_write_lock`](pacquet_fs::ensure_file) is
//! process-local (a static array of [`std::sync::Mutex<()>`] stripes
//! keyed by hashed path), just like upstream's
//! `locker: Map<string, number>`. The cross-process safety contract
//! therefore lives entirely in the kernel-level `O_CREAT | O_EXCL` +
//! atomic-rename primitives. The existing intra-process 32-thread
//! test in `ensure_file::tests`
//! ([`concurrent_writers_of_same_path_do_not_swap_the_inode`])
//! exercises the lock; this suite exercises the unprotected
//! filesystem-only path.

use sha2::{Digest, Sha512};
use std::{fs, path::Path, process::Command, sync::Arc, thread};
use tempfile::tempdir;

/// Number of worker processes per test. Eight matches upstream's
/// `numWorkers = 8` default.
const WORKER_COUNT: usize = 8;

/// 256 KiB matches upstream's `crypto.randomBytes(256 * 1024)`.
/// Big enough to make a partial write detectable as a size mismatch
/// without being so big it dominates the test runtime.
const CONTENT_SIZE: usize = 256 * 1024;

/// Path to the test-only worker binary that `cargo build` produced
/// alongside this integration test. The env var is set by Cargo at
/// test compile time so the worker stays reachable without anyone
/// having to run `cargo build --bin` first.
const WORKER_BIN: &str = env!("CARGO_BIN_EXE_cafs_stress_worker");

/// Run [`WORKER_COUNT`] worker subprocesses in parallel, each calling
/// `ensure_file(target, content, None)`. Returns the exit codes; the
/// caller asserts every one is `0`.
fn run_workers(content_path: &Path, target_path: &Path) -> Vec<std::process::ExitStatus> {
    let content_path: Arc<Path> = Arc::from(content_path.to_path_buf());
    let target_path: Arc<Path> = Arc::from(target_path.to_path_buf());

    #[expect(
        clippy::needless_collect,
        reason = "Collecting the handles is needed to spawn all worker subprocesses before joining them"
    )]
    let handles: Vec<_> = (0..WORKER_COUNT)
        .map(|_| {
            let content_path = Arc::clone(&content_path);
            let target_path = Arc::clone(&target_path);
            thread::spawn(move || {
                Command::new(WORKER_BIN)
                    .arg(&*content_path)
                    .arg(&*target_path)
                    .status()
                    .expect("spawn cafs_stress_worker")
            })
        })
        .collect();
    handles.into_iter().map(|handle| handle.join().expect("worker thread")).collect()
}

/// Sha-512-hex the byte slice, the same digest format
/// [`pacquet_fs::ensure_file`] would use for CAS naming.
fn sha512_hex(content: &[u8]) -> String {
    let digest = Sha512::digest(content);
    format!("{digest:x}")
}

/// Concurrent writes of the same buffer from many processes all
/// succeed and converge on a byte-identical CAS file. Mirrors
/// upstream's [`should handle concurrent writes from multiple
/// processes without corruption`](https://github.com/pnpm/pnpm/blob/8695496f58/store/cafs/test/writeBufferToCafs.test.ts#L60).
#[test]
fn multi_process_concurrent_writes_converge_on_correct_content() {
    let tmp = tempdir().expect("tempdir");
    let content_path = tmp.path().join("content.bin");
    let target_path = tmp.path().join("target.bin");

    let content: Vec<u8> = (0..CONTENT_SIZE).map(|i| (i % 256) as u8).collect();
    fs::write(&content_path, &content).expect("write content fixture");
    let expected_digest = sha512_hex(&content);

    let statuses = run_workers(&content_path, &target_path);

    for (idx, status) in statuses.iter().enumerate() {
        assert!(status.success(), "worker {idx} failed: {status:?}");
    }

    let final_content = fs::read(&target_path).expect("read target");
    assert_eq!(final_content.len(), content.len(), "size mismatch after concurrent writes");
    assert_eq!(
        sha512_hex(&final_content),
        expected_digest,
        "content mismatch after concurrent writes",
    );
}

/// A pre-seeded corrupt blob at the target path doesn't wedge the
/// store: every concurrent writer still returns success, and the
/// final on-disk content matches the buffer the workers were trying
/// to write. Mirrors upstream's [`should recover from a corrupt file
/// when multiple processes write
/// concurrently`](https://github.com/pnpm/pnpm/blob/8695496f58/store/cafs/test/writeBufferToCafs.test.ts#L85).
#[test]
fn multi_process_recovery_from_pre_seeded_corrupt_file() {
    let tmp = tempdir().expect("tempdir");
    let content_path = tmp.path().join("content.bin");
    let target_path = tmp.path().join("target.bin");

    let content: Vec<u8> = (0..CONTENT_SIZE).map(|i| ((i * 7) % 256) as u8).collect();
    fs::write(&content_path, &content).expect("write content fixture");
    let expected_digest = sha512_hex(&content);

    // Pre-seed a corrupt file at the target path. Same byte count
    // as the real content (so the size short-circuit doesn't fire)
    // but every byte inverted, so the byte-comparison in
    // `verify_or_rewrite` rejects it and `write_atomic` rewrites
    // the blob. `0xAA` is the bit-inverse of nothing in particular —
    // just a non-zero pattern that won't accidentally match the real
    // content even for trivial inputs.
    let corrupt = vec![0xAA_u8; CONTENT_SIZE];
    fs::write(&target_path, &corrupt).expect("pre-seed corrupt blob");

    let statuses = run_workers(&content_path, &target_path);

    for (idx, status) in statuses.iter().enumerate() {
        assert!(status.success(), "worker {idx} failed: {status:?}");
    }

    let final_content = fs::read(&target_path).expect("read target");
    assert_eq!(
        final_content.len(),
        content.len(),
        "size mismatch after recovery from pre-seeded corrupt file",
    );
    assert_eq!(
        sha512_hex(&final_content),
        expected_digest,
        "content mismatch after recovery from pre-seeded corrupt file",
    );
}

/// A pre-seeded truncated prefix of the correct content (simulating
/// a writer that crashed mid-`write_all`) also recovers: the size
/// mismatch short-circuits the byte comparison and `write_atomic`
/// renames a fresh blob over the partial one. Mirrors upstream's
/// [`should recover from a truncated file (simulating crash
/// mid-write)`](https://github.com/pnpm/pnpm/blob/8695496f58/store/cafs/test/writeBufferToCafs.test.ts#L111).
#[test]
fn multi_process_recovery_from_pre_seeded_truncated_file() {
    let tmp = tempdir().expect("tempdir");
    let content_path = tmp.path().join("content.bin");
    let target_path = tmp.path().join("target.bin");

    let content: Vec<u8> = (0..CONTENT_SIZE).map(|i| ((i * 13) % 256) as u8).collect();
    fs::write(&content_path, &content).expect("write content fixture");
    let expected_digest = sha512_hex(&content);

    // Pre-seed the first 1 KiB of the correct content. Same as
    // upstream's `content.subarray(0, 1024)`: matches the prefix
    // of what the workers will write, so a naive content-matching
    // path that compared only the first chunk would erroneously
    // claim a hit. The size-mismatch guard inside
    // `verify_or_rewrite` is what makes recovery work.
    fs::write(&target_path, &content[..1024]).expect("pre-seed truncated blob");

    let statuses = run_workers(&content_path, &target_path);

    for (idx, status) in statuses.iter().enumerate() {
        assert!(status.success(), "worker {idx} failed: {status:?}");
    }

    let final_content = fs::read(&target_path).expect("read target");
    assert_eq!(
        final_content.len(),
        content.len(),
        "size mismatch after recovery from pre-seeded truncated file",
    );
    assert_eq!(
        sha512_hex(&final_content),
        expected_digest,
        "content mismatch after recovery from pre-seeded truncated file",
    );
}
