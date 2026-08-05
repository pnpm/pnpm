use crate::StoreDir;
use sha2::{Digest, Sha512};
use std::path::PathBuf;

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
