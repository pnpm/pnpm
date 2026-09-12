use crate::{StoreDir, WriteCasFileFromReaderError};
use sha2::{Digest, Sha512};
use std::{io, path::PathBuf};

#[test]
fn cas_file_path() {
    fn case(file_content: &str, executable: bool, expected: &str) {
        eprintln!("CASE: {file_content:?}, {executable:?}");
        let store_dir = StoreDir::new("STORE_DIR");
        let file_hash = Sha512::digest(file_content);
        eprintln!("file_hash = {file_hash:x}");
        let received = store_dir.cas_file_path(file_hash, executable);
        let expected: PathBuf = expected.split('/').collect();
        assert_eq!(&received, &expected);
    }

    case(
        "hello world",
        false,
        "STORE_DIR/v11/files/30/9ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f",
    );

    case(
        "hello world",
        true,
        "STORE_DIR/v11/files/30/9ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f-exec",
    );
}

#[test]
fn cas_file_path_by_mode_suffix_matches_write_side() {
    // Tarballs frequently ship scripts as `0o744` (user-exec only).
    // The write side treats any-exec-bit-set as executable and stores
    // the blob under `-exec`; the read side must use the same rule,
    // otherwise every cache lookup for such a file turns into a miss.
    let store_dir = StoreDir::new("STORE_DIR");
    let hex = "a".repeat(128);
    for mode in [0o744, 0o755, 0o775, 0o100, 0o010, 0o001] {
        let path = store_dir
            .cas_file_path_by_mode(&hex, mode)
            .unwrap_or_else(|| panic!("mode {mode:o} should produce a path"));
        assert!(
            path.to_string_lossy().ends_with("-exec"),
            "mode {mode:o} should resolve to an `-exec` path, got {path:?}",
        );
    }
    for mode in [0o644, 0o600, 0o444, 0o000] {
        let path = store_dir
            .cas_file_path_by_mode(&hex, mode)
            .unwrap_or_else(|| panic!("mode {mode:o} should produce a path"));
        assert!(
            !path.to_string_lossy().ends_with("-exec"),
            "mode {mode:o} should NOT resolve to an `-exec` path, got {path:?}",
        );
    }
}

/// The shard-mkdir cache is empty on a fresh `StoreDir` (we
/// haven't called `init`) and grows as `write_cas_file` runs its
/// lazy fallback. This test pins three invariants:
///
/// * the first write into a given shard populates the cache entry
///   for that shard (no eager seeding);
/// * a second write of identical content is a successful noop via
///   `ensure_file`'s `AlreadyExists` → `verify_or_rewrite` path
///   (the `O_CREAT|O_EXCL` open returns `EEXIST`, `verify_or_rewrite`
///   byte-compares the existing file against the buffer and returns
///   `Ok(())` once they match — so the existing CAS blob is left
///   in place), and the cache is unchanged;
/// * a later write of different content still succeeds whether it
///   lands in the same shard or a new one.
///
/// Recovering from an out-of-band `rmdir` of a cached shard dir is
/// intentionally out of scope: pnpm's equivalent `dirs` Set in
/// `store/cafs/src/writeFile.ts` doesn't handle that either, and
/// the install aborts with the kernel's `open` error if it
/// happens.
#[test]
fn shard_cache_populates_on_first_write_and_skips_mkdir_thereafter() {
    use tempfile::tempdir;

    let tempdir = tempdir().unwrap();
    let store_dir = StoreDir::new(tempdir.path());

    let (path_a, hash_a) = store_dir.write_cas_file(b"hello world", false).unwrap();
    assert!(store_dir.shard_already_ensured(hash_a[0]));
    assert!(path_a.is_file());

    // Second write of identical content — same hash, same path —
    // hits `ensure_file`'s `AlreadyExists` → `verify_or_rewrite`
    // path: the `O_CREAT|O_EXCL` open returns `EEXIST`, then
    // `verify_or_rewrite` byte-compares the existing file against
    // the buffer, finds them equal, and returns `Ok(())` without
    // writing again. A torn-blob mismatch would route through
    // `write_atomic` instead, which is covered by
    // `existing_target_with_wrong_content_is_overwritten_atomically`
    // over in `crates/fs/src/ensure_file.rs`.
    let (path_b, hash_b) = store_dir.write_cas_file(b"hello world", false).unwrap();
    assert_eq!(hash_a, hash_b);
    assert_eq!(path_a, path_b);
    assert!(store_dir.shard_already_ensured(hash_b[0]));

    // Different content: either lands in a fresh shard (cache
    // grows by one) or happens to share the same first digest byte
    // as "hello world" (cache stays put). Either way the write
    // must succeed and materialize the file on disk.
    let (path_c, _) = store_dir.write_cas_file(b"goodbye world", false).unwrap();
    assert!(path_c.is_file());
}

/// The streaming writer must land the same bytes at the same
/// hash-derived path as the buffer-based writer — the two are used
/// interchangeably by tarball extraction, so any divergence in path,
/// hash, or on-disk content splits the CAS.
#[test]
fn write_cas_file_from_reader_matches_write_cas_file() {
    use tempfile::tempdir;

    // Larger than the internal copy buffer so the read loop runs more
    // than one iteration.
    let content: Vec<u8> = (0u32..200_000).flat_map(u32::to_le_bytes).collect();

    for executable in [false, true] {
        eprintln!("CASE: executable = {executable:?}");
        let buffered_dir = tempdir().unwrap();
        let buffered_store = StoreDir::new(buffered_dir.path());
        let (buffered_path, buffered_hash) =
            buffered_store.write_cas_file(&content, executable).unwrap();

        let streamed_dir = tempdir().unwrap();
        let streamed_store = StoreDir::new(streamed_dir.path());
        let (streamed_path, streamed_hash, streamed_size) = streamed_store
            .write_cas_file_from_reader(
                &mut content.as_slice(),
                executable,
                Some(content.len() as u64),
            )
            .unwrap();

        assert_eq!(streamed_hash, buffered_hash);
        assert_eq!(streamed_size, content.len() as u64);
        assert_eq!(
            streamed_path.strip_prefix(streamed_store.root()).unwrap(),
            buffered_path.strip_prefix(buffered_store.root()).unwrap(),
        );
        assert_eq!(std::fs::read(&streamed_path).unwrap(), content);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&streamed_path).unwrap().permissions().mode();
            assert_eq!(
                pnpm_fs::file_mode::is_executable(mode),
                executable,
                "streamed CAS file mode {mode:o} must match executable = {executable}",
            );
        }
    }
}

/// A live entry already at the target path is kept (same inode), and
/// the temp file the stream wrote is cleaned out of `files/` — leaking
/// it would accumulate one orphan per warm large-entry extraction.
#[test]
fn write_cas_file_from_reader_keeps_existing_live_entry_and_removes_temp() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let store_dir = StoreDir::new(dir.path());
    let content = b"streamed twice";

    let (first_path, _, _) = store_dir
        .write_cas_file_from_reader(&mut content.as_slice(), false, Some(content.len() as u64))
        .unwrap();
    let (second_path, _, second_size) = store_dir
        .write_cas_file_from_reader(&mut content.as_slice(), false, Some(content.len() as u64))
        .unwrap();

    assert_eq!(first_path, second_path);
    assert_eq!(second_size, content.len() as u64);
    assert_eq!(std::fs::read(&second_path).unwrap(), content);

    // Only the 2-hex-char shard directories may remain in `files/`.
    let stray: Vec<_> = std::fs::read_dir(store_dir.files_dir())
        .unwrap()
        .map(|dirent| dirent.unwrap())
        .filter(|dirent| !dirent.file_type().unwrap().is_dir())
        .map(|dirent| dirent.file_name())
        .collect();
    assert_eq!(stray, Vec::<std::ffi::OsString>::new(), "no temp files may leak into files/");
}

/// A same-length blob at the target whose bytes differ (disk
/// corruption, or a tampered store) must not be kept: the streamed
/// writer byte-compares before trusting an existing entry, matching
/// `ensure_file`'s guarantee for the buffered writer.
#[test]
fn write_cas_file_from_reader_replaces_same_length_corrupt_entry() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let store_dir = StoreDir::new(dir.path());
    let content = b"authentic cas payload";

    let (file_path, _, _) = store_dir
        .write_cas_file_from_reader(&mut content.as_slice(), false, Some(content.len() as u64))
        .unwrap();
    std::fs::write(&file_path, vec![b'X'; content.len()]).unwrap();

    let (second_path, _, _) = store_dir
        .write_cas_file_from_reader(&mut content.as_slice(), false, Some(content.len() as u64))
        .unwrap();

    assert_eq!(second_path, file_path);
    assert_eq!(std::fs::read(&file_path).unwrap(), content, "corrupt blob must be healed");
}

/// A shard-directory failure after the bytes have already streamed
/// must not leak the temp file into `files/`.
#[test]
fn write_cas_file_from_reader_removes_temp_when_shard_creation_fails() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let store_dir = StoreDir::new(dir.path());
    let content = b"shard blocked";

    // Occupy the shard path with a regular file so `create_dir_all`
    // for `files/XX/` fails after the stream completes.
    let shard = format!("{:02x}", Sha512::digest(content)[0]);
    std::fs::create_dir_all(store_dir.files_dir()).unwrap();
    let blocker = store_dir.files_dir().join(&shard);
    std::fs::write(&blocker, b"not a directory").unwrap();

    let err = store_dir
        .write_cas_file_from_reader(&mut content.as_slice(), false, Some(content.len() as u64))
        .expect_err("shard creation failure must propagate");
    assert!(
        matches!(err, WriteCasFileFromReaderError::Write(_)),
        "expected the Write variant, got: {err:?}",
    );

    let leftovers: Vec<_> = std::fs::read_dir(store_dir.files_dir())
        .unwrap()
        .map(|dirent| dirent.unwrap().file_name())
        .collect();
    assert_eq!(
        leftovers,
        vec![std::ffi::OsString::from(&shard)],
        "only the blocking file may remain — the stream temp must be removed",
    );
}

/// A reader that ends short of `expected_size` (a truncated archive)
/// must fail without committing anything to a content-addressed path —
/// the partial blob would be correctly addressed but must not enter
/// the store — and without leaking its temp file.
#[test]
fn write_cas_file_from_reader_rejects_short_reader_without_committing() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let store_dir = StoreDir::new(dir.path());
    let content = b"cut short";

    let err = store_dir
        .write_cas_file_from_reader(&mut content.as_slice(), false, Some(content.len() as u64 + 7))
        .expect_err("a short reader must not commit to the store");
    assert!(
        matches!(err, WriteCasFileFromReaderError::Read(_)),
        "expected the Read variant, got: {err:?}",
    );

    let leftovers: Vec<_> = std::fs::read_dir(store_dir.files_dir())
        .unwrap()
        .map(|dirent| dirent.unwrap().file_name())
        .collect();
    assert_eq!(
        leftovers,
        Vec::<std::ffi::OsString>::new(),
        "neither a CAS blob nor a temp file may remain after a short stream",
    );
}

/// A reader failure mid-stream surfaces as the `Read` variant (so the
/// tarball layer can attribute it to the archive, not the store) and
/// leaves no temp file behind.
#[test]
fn write_cas_file_from_reader_cleans_up_temp_on_reader_error() {
    use tempfile::tempdir;

    struct FailingReader;
    impl io::Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("simulated decode failure"))
        }
    }

    let dir = tempdir().unwrap();
    let store_dir = StoreDir::new(dir.path());

    let err = store_dir
        .write_cas_file_from_reader(&mut FailingReader, false, None)
        .expect_err("reader failure must propagate");
    assert!(
        matches!(err, WriteCasFileFromReaderError::Read(_)),
        "expected the Read variant, got: {err:?}",
    );

    let leftovers: Vec<_> = std::fs::read_dir(store_dir.files_dir())
        .unwrap()
        .map(|dirent| dirent.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, Vec::<std::ffi::OsString>::new(), "failed stream must remove its temp");
}

#[test]
fn cas_file_path_by_mode_rejects_invalid_hex() {
    let store_dir = StoreDir::new("STORE_DIR");
    assert_eq!(store_dir.cas_file_path_by_mode("", 0o644), None);
    assert_eq!(store_dir.cas_file_path_by_mode("a", 0o644), None);
    // Exactly two hex chars is still rejected — it would resolve to
    // the shard directory itself (files/XX/), which is not a file.
    assert_eq!(store_dir.cas_file_path_by_mode("ab", 0o644), None);
    assert_eq!(store_dir.cas_file_path_by_mode("zz", 0o644), None);
    assert_eq!(store_dir.cas_file_path_by_mode("Ab\tcd", 0o644), None);
    assert!(store_dir.cas_file_path_by_mode("abc", 0o644).is_some());
    assert!(store_dir.cas_file_path_by_mode("abcdef", 0o755).is_some());
}
