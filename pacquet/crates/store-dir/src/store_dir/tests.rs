use super::{STORE_VERSION, StoreDir};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use std::path::{Path, PathBuf};

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

/// `StoreDir::from(PathBuf)` appends [`STORE_VERSION`] to any path
/// that doesn't already end with it — matching pnpm's
/// [`getStorePath`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L39-L42)
/// branch. Both the auto-append happy path and the
/// already-suffixed idempotent path are pinned here so a regression
/// would surface as either a missing `v11` (the original bug — pnpm
/// rejects the resulting `.modules.yaml` with
/// `ERR_PNPM_UNEXPECTED_STORE`) or a duplicated `v11/v11` segment.
#[test]
fn from_pathbuf_auto_appends_store_version_when_missing() {
    let store = StoreDir::from(PathBuf::from("/home/user/.local/share/pnpm/store"));
    assert_eq!(store.root(), Path::new("/home/user/.local/share/pnpm/store/v11"));
}

#[test]
fn from_pathbuf_does_not_double_append_when_already_suffixed() {
    let store = StoreDir::from(PathBuf::from("/home/user/.local/share/pnpm/store/v11"));
    assert_eq!(store.root(), Path::new("/home/user/.local/share/pnpm/store/v11"));
}

/// Round-trip the `storeDir` string pacquet writes to `.modules.yaml`
/// against the pnpm comparison contract: pnpm rebuilds `<X>/v11`
/// from the user's home and demands an exact match against the
/// recorded value. The constant test makes the bug
/// `ERR_PNPM_UNEXPECTED_STORE` triggered legible for future readers.
///
/// Build the expected value through `Path::join` rather than a
/// hardcoded `/v11` so the assertion stays valid on Windows: pnpm
/// uses Node's [`path.join`](https://nodejs.org/api/path.html#pathjoinpaths)
/// (and [`getStorePath`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L39-L42)
/// goes through it too), which emits `\v11` on Windows; pacquet's
/// [`From<PathBuf> for StoreDir`] mirrors that with `PathBuf::join`.
/// Hardcoding `/` here would compare a backslash-joined left against
/// a slash-joined right and panic on Windows only.
#[test]
fn modules_yaml_serialized_store_dir_carries_store_version() {
    let store = StoreDir::new("/tmp/.pnpm-store");
    let recorded = store.display().to_string();
    let pnpm_would_emit = Path::new("/tmp/.pnpm-store").join(STORE_VERSION).display().to_string();
    assert_eq!(recorded, pnpm_would_emit);
}

#[test]
fn deserialize_applies_store_version_to_unsuffixed_path() {
    let json = r#""/home/user/.local/share/pnpm/store""#;
    let store: StoreDir = serde_json::from_str(json).expect("deserialize StoreDir");
    assert_eq!(store.root(), Path::new("/home/user/.local/share/pnpm/store/v11"));
}

#[test]
fn deserialize_preserves_already_suffixed_path() {
    let json = r#""/home/user/.local/share/pnpm/store/v11""#;
    let store: StoreDir = serde_json::from_str(json).expect("deserialize StoreDir");
    assert_eq!(store.root(), Path::new("/home/user/.local/share/pnpm/store/v11"));
}

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
