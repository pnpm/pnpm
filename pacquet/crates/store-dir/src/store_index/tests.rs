use super::{
    CafsFileInfo, GET_MANY_CHUNK, PackageFilesIndex, StoreIndex, git_hosted_store_index_key,
    pick_store_index_key, store_index_key,
};
use crate::StoreDir;
use pretty_assertions::assert_eq;
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
    files.insert(
        "index.js".to_string(),
        CafsFileInfo { checked_at: None, digest: "def".to_string(), mode: 0o755, size: 42 },
    );
    PackageFilesIndex {
        manifest: None,
        requires_build: Some(false),
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    }
}

#[test]
fn key_format_is_integrity_tab_pkg_id() {
    assert_eq!(store_index_key("sha512-abc", "lodash@4.17.21"), "sha512-abc\tlodash@4.17.21");
}

#[test]
fn git_hosted_key_uses_built_marker() {
    // Mirrors upstream's `gitHostedStoreIndexKey(pkgId, { built })`.
    assert_eq!(
        git_hosted_store_index_key("github.com/foo/bar/abc1234", true),
        "github.com/foo/bar/abc1234\tbuilt",
    );
    assert_eq!(
        git_hosted_store_index_key("github.com/foo/bar/abc1234", false),
        "github.com/foo/bar/abc1234\tnot-built",
    );
}

#[test]
fn pick_store_index_key_uses_integrity_for_plain_tarball() {
    let key = pick_store_index_key(Some("sha512-abc"), false, "foo@1.0.0", true);
    assert_eq!(key, "sha512-abc\tfoo@1.0.0");
}

#[test]
fn pick_store_index_key_uses_git_hosted_for_flagged_tarball() {
    // Even with integrity present, `git_hosted = true` routes to the
    // built/not-built key — the built dimension is what disambiguates two
    // cached variants of the same hosted tarball.
    let key = pick_store_index_key(Some("sha512-abc"), true, "github.com/foo/bar/abc1234", true);
    assert_eq!(key, "github.com/foo/bar/abc1234\tbuilt");

    let key = pick_store_index_key(Some("sha512-abc"), true, "github.com/foo/bar/abc1234", false);
    assert_eq!(key, "github.com/foo/bar/abc1234\tnot-built");
}

#[test]
fn pick_store_index_key_uses_git_hosted_for_missing_integrity() {
    // pnpm falls through to `gitHostedStoreIndexKey` for any resolution
    // missing integrity — covers bare `type: git` resolutions and old
    // lockfile entries that predate integrity for tarballs.
    let key = pick_store_index_key(None, false, "github.com/foo/bar/abc1234", true);
    assert_eq!(key, "github.com/foo/bar/abc1234\tbuilt");
}

#[test]
fn set_then_get_round_trips() {
    let dir = tempdir().unwrap();
    let idx = StoreIndex::open(dir.path()).unwrap();
    let key = store_index_key("sha512-xyz", "pkg@1.0.0");
    let original = sample_index();

    idx.set(&key, &original).unwrap();
    let loaded = idx.get(&key).unwrap().expect("row must exist after set");

    assert_eq!(loaded, original);
}

#[test]
fn get_returns_none_for_missing_key() {
    let dir = tempdir().unwrap();
    let idx = StoreIndex::open(dir.path()).unwrap();
    assert!(idx.get("sha512-never\tnone@0.0.0").unwrap().is_none());
    assert!(!idx.contains_key("sha512-never\tnone@0.0.0").unwrap());
}

#[test]
fn set_is_upsert() {
    let dir = tempdir().unwrap();
    let idx = StoreIndex::open(dir.path()).unwrap();
    let key = store_index_key("sha512-abc", "pkg@1.0.0");

    let first = sample_index();
    idx.set(&key, &first).unwrap();

    let mut second = sample_index();
    second.algo = "sha256".to_string();
    idx.set(&key, &second).unwrap();

    let loaded = idx.get(&key).unwrap().unwrap();
    assert_eq!(loaded.algo, "sha256");
}

#[test]
fn reopening_the_same_db_sees_prior_writes() {
    let dir = tempdir().unwrap();
    let key = store_index_key("sha512-abc", "pkg@1.0.0");
    let payload = sample_index();

    {
        let idx = StoreIndex::open(dir.path()).unwrap();
        idx.set(&key, &payload).unwrap();
    }

    let idx = StoreIndex::open(dir.path()).unwrap();
    assert_eq!(idx.get(&key).unwrap().unwrap(), payload);
}

#[test]
fn index_db_lives_at_store_dir_v11() {
    let root = tempdir().unwrap();
    let store = StoreDir::new(root.path());
    let idx = StoreIndex::open_in(&store).unwrap();
    idx.set("k\tv", &sample_index()).unwrap();
    assert!(store.root().join("index.db").exists());
}

/// A row whose bytes are msgpackr-records (as pnpm writes) must decode
/// through `StoreIndex::get` just like a pacquet-written row. The
/// fixture here is the same "one-file index" bytes used in the
/// `msgpackr_records` unit tests — inserted via a direct SQL write so
/// we test the decoder *through the get path*, not the round-trip.
#[test]
fn get_decodes_msgpackr_records_rows() {
    let dir = tempdir().unwrap();
    let idx = StoreIndex::open(dir.path()).unwrap();
    let key = "sha512-xyz\tfake@1.0.0";

    // Captured from `node /tmp/msgpackr_fixture.mjs`, "one-file index".
    let msgpackr_row: &[u8] = &[
        0xd4, 0x72, 0x40, 0x92, 0xa4, 0x61, 0x6c, 0x67, 0x6f, 0xa5, 0x66, 0x69, 0x6c, 0x65, 0x73,
        0xa6, 0x73, 0x68, 0x61, 0x35, 0x31, 0x32, 0x81, 0xac, 0x70, 0x61, 0x63, 0x6b, 0x61, 0x67,
        0x65, 0x2e, 0x6a, 0x73, 0x6f, 0x6e, 0xd4, 0x72, 0x41, 0x94, 0xa6, 0x64, 0x69, 0x67, 0x65,
        0x73, 0x74, 0xa4, 0x6d, 0x6f, 0x64, 0x65, 0xa4, 0x73, 0x69, 0x7a, 0x65, 0xa9, 0x63, 0x68,
        0x65, 0x63, 0x6b, 0x65, 0x64, 0x41, 0x74, 0xa3, 0x61, 0x62, 0x63, 0xcd, 0x01, 0xa4, 0x11,
        0xcb, 0x42, 0x78, 0xbc, 0xfe, 0x56, 0x80, 0x00, 0x00,
    ];
    idx.conn
        .execute(
            "INSERT INTO package_index (key, data) VALUES (?1, ?2)",
            rusqlite::params![key, msgpackr_row],
        )
        .unwrap();

    let loaded = idx.get(key).unwrap().expect("row must decode");
    assert_eq!(loaded.algo, "sha512");
    let info = loaded.files.get("package.json").unwrap();
    assert_eq!(info.digest, "abc");
    assert_eq!(info.mode, 0o644);
    assert_eq!(info.size, 17);
    assert_eq!(info.checked_at, Some(1_700_000_000_000));
}

#[test]
fn get_many_returns_empty_for_empty_input() {
    let dir = tempdir().unwrap();
    let idx = StoreIndex::open(dir.path()).unwrap();
    idx.set(&store_index_key("sha512-a", "x@1.0.0"), &sample_index()).unwrap();

    let out = idx.get_many(&[]).unwrap();
    assert!(out.is_empty());
}

#[test]
fn get_many_all_miss_returns_empty_map() {
    let dir = tempdir().unwrap();
    let idx = StoreIndex::open(dir.path()).unwrap();
    let keys = vec![
        store_index_key("sha512-a", "missing-a@1.0.0"),
        store_index_key("sha512-b", "missing-b@1.0.0"),
        store_index_key("sha512-c", "missing-c@1.0.0"),
    ];

    let out = idx.get_many(&keys).unwrap();
    assert!(out.is_empty());
}

#[test]
fn get_many_all_hit_returns_every_row() {
    let dir = tempdir().unwrap();
    let idx = StoreIndex::open(dir.path()).unwrap();
    let payload = sample_index();
    let keys: Vec<String> =
        (0..5).map(|index| store_index_key("sha512-x", &format!("pkg{index}@1.0.0"))).collect();
    for key in &keys {
        idx.set(key, &payload).unwrap();
    }

    let out = idx.get_many(&keys).unwrap();
    assert_eq!(out.len(), keys.len());
    for key in &keys {
        assert_eq!(out.get(key), Some(&payload));
    }
}

#[test]
fn get_many_mixed_hit_and_miss_returns_only_hits() {
    let dir = tempdir().unwrap();
    let idx = StoreIndex::open(dir.path()).unwrap();
    let payload = sample_index();
    let hit_keys: Vec<String> =
        (0..3).map(|index| store_index_key("sha512-h", &format!("hit{index}@1.0.0"))).collect();
    let miss_keys: Vec<String> =
        (0..3).map(|index| store_index_key("sha512-m", &format!("miss{index}@1.0.0"))).collect();
    for key in &hit_keys {
        idx.set(key, &payload).unwrap();
    }

    let mut all_keys = hit_keys.clone();
    all_keys.extend(miss_keys.clone());
    let out = idx.get_many(&all_keys).unwrap();

    assert_eq!(out.len(), hit_keys.len());
    for key in &hit_keys {
        assert!(out.contains_key(key), "hit key missing from result: {key}");
    }
    for key in &miss_keys {
        assert!(!out.contains_key(key), "miss key present in result: {key}");
    }
}

/// A row whose bytes don't decode (corruption, foreign writer) must
/// be skipped without failing the batch. `load_cached_cas_paths`
/// already does `.ok()?` on the per-key path, treating decode
/// errors as cache misses; the batched read keeps that semantic,
/// though `get_many` emits a `debug!` log for the dropped row.
#[test]
fn get_many_skips_undecodable_rows() {
    let dir = tempdir().unwrap();
    let idx = StoreIndex::open(dir.path()).unwrap();
    let payload = sample_index();
    let good_key = store_index_key("sha512-good", "good@1.0.0");
    let bad_key = store_index_key("sha512-bad", "bad@1.0.0");
    idx.set(&good_key, &payload).unwrap();
    idx.conn
        .execute(
            "INSERT INTO package_index (key, data) VALUES (?1, ?2)",
            // Bytes that aren't valid msgpack — the decoder will reject
            // these and `get_many` should drop them rather than fail
            // the whole batch.
            rusqlite::params![&bad_key, &b"not msgpack"[..]],
        )
        .unwrap();

    let out = idx.get_many(&[good_key.clone(), bad_key.clone()]).unwrap();

    assert_eq!(out.len(), 1);
    assert!(out.contains_key(&good_key));
    assert!(!out.contains_key(&bad_key));
}

/// Exercise the chunking path with more keys than `GET_MANY_CHUNK`.
/// `SQLite`'s `INSERT OR REPLACE` is fast enough that seeding a few
/// thousand rows in-process stays cheap.
#[test]
fn get_many_handles_more_keys_than_chunk_size() {
    let dir = tempdir().unwrap();
    let mut idx = StoreIndex::open(dir.path()).unwrap();
    let payload = sample_index();
    let total = GET_MANY_CHUNK + 100;
    let keys: Vec<String> = (0..total)
        .map(|index| store_index_key("sha512-c", &format!("chunked{index}@1.0.0")))
        .collect();
    let entries = keys.iter().map(|key| (key.clone(), sample_index()));
    idx.set_many(entries).unwrap();

    let out = idx.get_many(&keys).unwrap();

    assert_eq!(out.len(), total);
    for key in &keys {
        assert_eq!(out.get(key), Some(&payload));
    }
}
