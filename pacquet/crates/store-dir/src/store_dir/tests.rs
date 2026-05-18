use super::StoreDir;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use std::path::Path;

#[test]
fn file_path_by_head_tail() {
    let received = "/home/user/.local/share/pnpm/store"
        .pipe(StoreDir::new)
        .file_path_by_head_tail("3e", "f722d37b016c63ac0126cfdcec");
    let expected =
        Path::new("/home/user/.local/share/pnpm/store/v11/files/3e/f722d37b016c63ac0126cfdcec");
    assert_eq!(received, expected);
}

#[test]
fn tmp() {
    let received = StoreDir::new("/home/user/.local/share/pnpm/store").tmp();
    let expected = Path::new("/home/user/.local/share/pnpm/store/v11/tmp");
    assert_eq!(received, expected);
}

/// `init` on a fresh store should materialize `v11/files/00..ff`
/// and populate the shard cache so later `write_cas_file` calls
/// can skip their lazy mkdir.
#[test]
fn init_creates_all_256_shards_and_populates_cache() {
    use tempfile::tempdir;

    let tempdir = tempdir().unwrap();
    let store = StoreDir::new(tempdir.path());
    store.init().unwrap();

    let files = tempdir.path().join("v11/files");
    assert!(files.is_dir(), "v11/files must exist after init");
    for shard in 0u8..=255 {
        let name = format!("{shard:02x}");
        assert!(files.join(&name).is_dir(), "shard {name} must exist after init");
        assert!(
            store.shard_already_ensured(shard),
            "shard {name} must be marked ensured in the cache",
        );
    }
}

/// `init` on a store where `files/` already exists must be a
/// near-noop: don't re-create anything, don't seed the cache. A
/// store created by an older pacquet might be missing shard dirs
/// we never materialized, and pre-seeding the cache in that case
/// would let `write_cas_file` skip `ensure_parent_dir` and blow up
/// at `open`. Leaving the cache empty keeps the lazy fallback in
/// `write_cas_file` responsible for materializing each shard the
/// first time it's written, matching pnpm's `writeFile.ts` `dirs`
/// Set.
/// If `v11/files/` is present but isn't a directory (store
/// corruption — a regular file landed there somehow), `init` must
/// surface a clear `io::Error` rather than silently becoming a noop
/// and letting each later `write_cas_file` fail with a less
/// actionable per-file `open` error. `create_dir_all` on a path
/// where a component is already a regular file returns an error
/// from the OS; we just need the gate to be tight enough to let it
/// run.
#[test]
fn init_rejects_non_directory_files_path() {
    use tempfile::tempdir;

    let tempdir = tempdir().unwrap();
    let v11 = tempdir.path().join("v11");
    std::fs::create_dir_all(&v11).unwrap();
    std::fs::write(v11.join("files"), b"i am not a directory").unwrap();

    let store = StoreDir::new(tempdir.path());
    // Don't pin the exact ErrorKind — platforms differ
    // (`NotADirectory` on Linux, `AlreadyExists` / `Uncategorized`
    // elsewhere). `expect_err` asserting that *an* error surfaced
    // is enough; the caller has already wired it through `warn!`.
    store.init().expect_err("init must fail when files/ isn't a directory");
    for shard in 0u8..=255 {
        assert!(
            !store.shard_already_ensured(shard),
            "a failing init must not seed the shard cache",
        );
    }
}

#[test]
fn init_warm_store_is_noop_and_leaves_cache_empty() {
    use tempfile::tempdir;

    let tempdir = tempdir().unwrap();
    let files = tempdir.path().join("v11/files");
    std::fs::create_dir_all(&files).unwrap();
    // Plant a sentinel inside a pre-existing shard so we can prove
    // init didn't wipe or re-create it. Plain `mkdir` of an existing
    // dir would fail anyway (EEXIST), but an aggressive port could
    // accidentally `remove_dir_all` + recreate, so pin the
    // invariant.
    let shard = files.join("00");
    std::fs::create_dir(&shard).unwrap();
    std::fs::write(shard.join("sentinel"), b"do not delete me").unwrap();

    let store = StoreDir::new(tempdir.path());
    store.init().unwrap();

    assert!(shard.join("sentinel").is_file(), "pre-existing shard content must survive init");
    for shard in 0u8..=255 {
        assert!(
            !store.shard_already_ensured(shard),
            "shard {shard:02x} must NOT be marked ensured on warm-store init — the cache is populated lazily from write_cas_file",
        );
    }
}
