use super::{
    EnsureFileError, ensure_file, file_equals_bytes, is_transient_rename_error, rename_with_retry,
    temp_path_for,
};
use std::{fs, io, path::Path};
use tempfile::tempdir;

#[cfg(unix)]
use super::{EMFILE, ENFILE, retry_on_fd_pressure};

#[test]
fn writes_a_new_file() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("new.txt");

    ensure_file(&path, b"hello", None).expect("new-file write succeeds");

    assert_eq!(fs::read(&path).unwrap(), b"hello");
}

#[test]
fn existing_target_with_matching_content_is_preserved() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("existing.txt");
    fs::write(&path, b"same").unwrap();

    ensure_file(&path, b"same", None).expect("matching contents should short-circuit");

    assert_eq!(fs::read(&path).unwrap(), b"same");
}

#[test]
fn existing_target_with_wrong_content_is_overwritten_atomically() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("torn.txt");
    fs::write(&path, b"garbage-from-crashed-prior-install").unwrap();

    ensure_file(&path, b"fresh", None).expect("torn blob should be rewritten");

    assert_eq!(fs::read(&path).unwrap(), b"fresh");
    let siblings: Vec<_> =
        fs::read_dir(tmp.path()).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(siblings, vec![std::ffi::OsString::from("torn.txt")]);
}

#[test]
fn missing_parent_dir_errors() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("nested/does/not/exist/file.txt");

    let err = ensure_file(&path, b"x", None).expect_err("missing parent should fail");
    match err {
        EnsureFileError::CreateFile { error, .. } => {
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
        }
        other => panic!("expected CreateFile/NotFound, got {other:?}"),
    }
}

/// Unix mode is honoured on the new-file path. Skipped on Windows
/// where the `mode` argument is `#[cfg_attr(windows, allow(unused))]`.
///
/// Asserts the **owner** bits specifically rather than the full
/// `0o777` triplet because `OpenOptionsExt::mode` runs through the
/// process umask, which strips group / other bits on systems with
/// a restrictive default (e.g. `umask 0o077` CI shells). Owner
/// bits are preserved under every sensible umask, so pinning just
/// those keeps the test robust without weakening what it verifies
/// (that `mode` is being threaded through to the syscall at all
/// and that the owner-exec bit survives — the observable property
/// that distinguishes an executable CAS blob from a data blob).
#[cfg(unix)]
#[test]
fn unix_mode_is_applied_on_new_files() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let path = tmp.path().join("exec.sh");

    ensure_file(&path, b"#!/bin/sh\n", Some(0o755)).expect("mode-honouring write");

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o700;
    assert_eq!(mode, 0o700, "owner rwx bits of 0o755 must survive any reasonable umask");
}

#[test]
fn temp_path_strips_exec_suffix() {
    let store_path = Path::new("/tmp/store/v11/files/ab/cdef-exec");
    let tmp = temp_path_for(store_path);
    let name = tmp.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("cdefx"), "got {name}");
}

#[test]
fn temp_path_passes_plain_basename_through() {
    let store_path = Path::new("/tmp/store/v11/files/ab/cdef");
    let tmp = temp_path_for(store_path);
    let name = tmp.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("cdef"), "got {name}");
    assert_ne!(name, "cdef", "must include pid + counter suffix");
}

/// Windows AV / indexer interference surfaces as
/// `PermissionDenied` or `ResourceBusy` and must trigger the
/// retry loop there. On non-Windows those codes are essentially
/// always permanent (permission / mount-point issues), so the
/// classifier must return `false` to avoid pathologically
/// spinning for 60 s on a misconfigured store dir. Any other
/// kind must propagate immediately on every platform.
#[test]
fn transient_rename_error_classifier() {
    let permission_denied = io::Error::from(io::ErrorKind::PermissionDenied);
    let resource_busy = io::Error::from(io::ErrorKind::ResourceBusy);

    #[cfg(windows)]
    {
        assert!(is_transient_rename_error(&permission_denied));
        assert!(is_transient_rename_error(&resource_busy));
    }
    #[cfg(not(windows))]
    {
        assert!(
            !is_transient_rename_error(&permission_denied),
            "Unix PermissionDenied is permanent, must not retry",
        );
        assert!(
            !is_transient_rename_error(&resource_busy),
            "Unix ResourceBusy is effectively permanent, must not retry",
        );
    }

    // Non-transient kinds must never trigger the retry loop on
    // any platform — a regression classifying e.g. `NotFound` as
    // transient would spin for 60 s on a legitimately missing
    // source.
    for kind in [
        io::ErrorKind::NotFound,
        io::ErrorKind::AlreadyExists,
        io::ErrorKind::InvalidInput,
        io::ErrorKind::InvalidData,
        io::ErrorKind::Unsupported,
        io::ErrorKind::Other,
    ] {
        assert!(
            !is_transient_rename_error(&io::Error::from(kind)),
            "{kind:?} must not be classified as transient",
        );
    }
}

/// A symlink at the target path — which on Unix returns `EEXIST`
/// from `open(O_CREAT|O_EXCL)` just like a regular file would —
/// must be scrubbed and replaced with a real regular file even
/// when its target's bytes match what we were about to write.
/// Leaving the symlink in place would fool downstream
/// `fs::hard_link` (which hardlinks the symlink itself on Linux,
/// not the target) and leak non-regular dirents into the CAS.
#[cfg(unix)]
#[test]
fn symlink_at_cas_path_is_scrubbed_to_a_regular_file() {
    let tmp = tempdir().unwrap();
    let real_target = tmp.path().join("other_real_file");
    fs::write(&real_target, b"payload").unwrap();

    let cas_path = tmp.path().join("cas_entry");
    std::os::unix::fs::symlink(&real_target, &cas_path).unwrap();

    ensure_file(&cas_path, b"payload", None).expect("symlink should be scrubbed");

    let meta = fs::symlink_metadata(&cas_path).unwrap();
    assert!(
        meta.file_type().is_file(),
        "cas_path must be a regular file after scrub, got {:?}",
        meta.file_type(),
    );
    assert_eq!(fs::read(&cas_path).unwrap(), b"payload");
    assert_eq!(fs::read(&real_target).unwrap(), b"payload");
}

#[cfg(unix)]
#[test]
fn dangling_symlink_at_cas_path_is_scrubbed_to_a_regular_file() {
    let tmp = tempdir().unwrap();
    let cas_path = tmp.path().join("cas_entry");
    std::os::unix::fs::symlink(tmp.path().join("nonexistent"), &cas_path).unwrap();

    ensure_file(&cas_path, b"fresh", None).expect("dangling link should be scrubbed");

    let meta = fs::symlink_metadata(&cas_path).unwrap();
    assert!(meta.file_type().is_file(), "cas_path must end as a regular file");
    assert_eq!(fs::read(&cas_path).unwrap(), b"fresh");
}

/// Happy-path rename (no transient errors) moves the payload
/// atomically and removes the source. Correctness only — we
/// deliberately don't assert a wall-clock bound because rename
/// latency on loaded CI / slow filesystems can exceed any
/// reasonable timing threshold without the retry path actually
/// being taken.
#[test]
fn rename_with_retry_succeeds_when_no_error() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    fs::write(&src, b"payload").unwrap();

    rename_with_retry(&src, &dst).expect("rename should succeed");

    assert_eq!(fs::read(&dst).unwrap(), b"payload");
    assert!(!src.exists(), "source should be gone after rename");
}

#[test]
fn file_equals_bytes_classifies_match_mismatch_and_length_mismatch() {
    let tmp = tempdir().unwrap();

    let equal = tmp.path().join("equal");
    fs::write(&equal, b"hello world").unwrap();
    assert!(file_equals_bytes(&equal, b"hello world").unwrap());

    let content_diff = tmp.path().join("content_diff");
    fs::write(&content_diff, b"hello world").unwrap();
    assert!(!file_equals_bytes(&content_diff, b"hello WORLD").unwrap());

    // `verify_or_rewrite`'s size-check short-circuits before
    // reaching this function in practice, but the function
    // itself still has to classify correctly if called directly.
    let length_diff = tmp.path().join("length_diff");
    fs::write(&length_diff, b"short").unwrap();
    assert!(!file_equals_bytes(&length_diff, b"longer payload").unwrap());
}

/// Multi-chunk files exercise the inner `read_exact` loop rather
/// than landing entirely in the first 8 KB read. Guards against
/// off-by-one regressions in the chunk-offset math, and confirms
/// a byte flipped in the *last* chunk isn't masked by an early
/// "first-chunk-matched" short-circuit.
#[test]
fn file_equals_bytes_handles_multi_chunk_files() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("big");

    // 20 KB: at least three 8 KB chunks.
    let content: Vec<u8> = (0..20_000).map(|index| (index % 251) as u8).collect();
    fs::write(&path, &content).unwrap();

    assert!(file_equals_bytes(&path, &content).unwrap());

    let mut perturbed = content;
    *perturbed.last_mut().unwrap() ^= 0xff;
    assert!(!file_equals_bytes(&path, &perturbed).unwrap());
}

#[cfg(unix)]
#[test]
fn retry_on_fd_pressure_retries_emfile_and_enfile_until_success() {
    for errno in [EMFILE, ENFILE] {
        let attempts = std::cell::Cell::new(0);
        let result = retry_on_fd_pressure(|| {
            let attempt = attempts.get();
            attempts.set(attempt + 1);
            if attempt < 2 { Err(io::Error::from_raw_os_error(errno)) } else { Ok("ok") }
        });
        assert_eq!(result.unwrap(), "ok");
        assert_eq!(attempts.get(), 3, "errno {errno} should have been retried twice");
    }
}

/// Concurrent writers of the same CAS path on a fresh dirent must
/// all return `Ok(())`, produce identical inode observations, and
/// leave a file with the correct content. One writer wins
/// `O_CREAT|O_EXCL`; the rest take `verify_or_rewrite`. With the
/// per-path mutex, the late-comers see the winner's fully-written
/// file and take the byte-match fast path, so the inode never
/// changes. Without it, a late-comer can race into `write_atomic`
/// on a partial size, swap the inode, and the per-writer
/// observations below can diverge under a multi-rename race.
///
/// Note this is a smoke test, not a strict regression test: any
/// observation taken *after* `ensure_file` returns has already
/// missed the rename window, so a single-rename race typically
/// converges on one final inode and slips past. It catches the
/// multi-rename case and validates the "no deadlock, all writers
/// see correct content" baseline.
#[cfg(unix)]
#[test]
fn concurrent_writers_of_same_path_do_not_swap_the_inode() {
    use std::os::unix::fs::MetadataExt;
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;

    let tmp = tempdir().unwrap();
    let path = Arc::new(tmp.path().join("shared"));
    let content: Arc<Vec<u8>> = Arc::new(vec![0xAB; 1024 * 64]);

    const WRITER_COUNT: usize = 32;
    let barrier = Arc::new(Barrier::new(WRITER_COUNT));
    let observed: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::with_capacity(WRITER_COUNT)));

    let handles: Vec<_> = (0..WRITER_COUNT)
        .map(|_| {
            let path = Arc::clone(&path);
            let content = Arc::clone(&content);
            let barrier = Arc::clone(&barrier);
            let observed = Arc::clone(&observed);
            thread::spawn(move || {
                barrier.wait();
                ensure_file(&path, &content, None).expect("each writer should succeed");
                let ino = fs::metadata(&*path).unwrap().ino();
                observed.lock().unwrap().push(ino);
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("writer thread should not panic");
    }

    let final_meta = fs::metadata(&*path).unwrap();
    assert_eq!(fs::read(&*path).unwrap(), *content);
    assert_eq!(final_meta.len(), content.len() as u64);
    let observed = observed.lock().unwrap();
    let first = observed[0];
    assert!(
        observed.iter().all(|ino| *ino == first),
        "inode changed during concurrent writes: {observed:?}",
    );
    assert_eq!(final_meta.ino(), first);
}

#[cfg(unix)]
#[test]
fn retry_on_fd_pressure_propagates_non_fd_errors() {
    let attempts = std::cell::Cell::new(0);
    let result: io::Result<()> = retry_on_fd_pressure(|| {
        attempts.set(attempts.get() + 1);
        Err(io::Error::from(io::ErrorKind::NotFound))
    });
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    assert_eq!(attempts.get(), 1, "non-fd-pressure errors must not retry");
}
