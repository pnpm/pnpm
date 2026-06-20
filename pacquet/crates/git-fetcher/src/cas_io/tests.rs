use super::{
    GitFetcherError, cas_path_digest, join_checked, materialize_into, synthesize_files_index,
};
use pacquet_store_dir::StoreDir;
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

fn assert_invalid_input(err: GitFetcherError) {
    match err {
        GitFetcherError::Io(io_err) => {
            assert_eq!(io_err.kind(), io::ErrorKind::InvalidInput);
        }
        other => panic!("expected Io(InvalidInput), got {other:?}"),
    }
}

#[test]
fn join_checked_accepts_normal_segments() {
    let root = Path::new("/root");
    let joined = join_checked(root, "a/b/c.txt").unwrap();
    // Use components() so the assertion stays platform-agnostic.
    let expected: Vec<_> = Path::new("/root/a/b/c.txt").components().collect();
    let actual: Vec<_> = joined.components().collect();
    assert_eq!(actual, expected);
}

#[test]
fn join_checked_strips_current_dir_components() {
    let root = Path::new("/root");
    let joined = join_checked(root, "./a").unwrap();
    let expected: Vec<_> = Path::new("/root/a").components().collect();
    let actual: Vec<_> = joined.components().collect();
    assert_eq!(actual, expected);
}

#[test]
fn join_checked_rejects_absolute_paths() {
    assert_invalid_input(join_checked(Path::new("/root"), "/etc/passwd").unwrap_err());
}

#[test]
fn join_checked_rejects_parent_dir() {
    assert_invalid_input(join_checked(Path::new("/root"), "../escape").unwrap_err());
    // Even a `..` deep in the path must be refused — otherwise
    // `a/../../escape` would slip through.
    assert_invalid_input(join_checked(Path::new("/root"), "a/../escape").unwrap_err());
}

#[test]
fn cas_path_digest_round_trips_through_write_cas_file() {
    // Anchor the digest-reconstruction logic against the canonical
    // write side: whatever `write_cas_file` produces, the read
    // side has to invert. A mismatch would silently corrupt the
    // `files_index` rows the fast path queues.
    let cas_root = tempdir().unwrap();
    let store_dir = StoreDir::from(cas_root.path().to_path_buf());

    let (regular_path, regular_hash) = store_dir.write_cas_file(b"hello", false).unwrap();
    assert_eq!(
        cas_path_digest(&regular_path).expect("round-trip non-exec"),
        format!("{regular_hash:x}"),
    );

    let (exec_path, exec_hash) = store_dir.write_cas_file(b"#!/bin/sh\n", true).unwrap();
    let digest = cas_path_digest(&exec_path).expect("round-trip exec");
    assert_eq!(digest, format!("{exec_hash:x}"), "`-exec` suffix must be stripped before parse");
}

#[test]
fn cas_path_digest_rejects_malformed_paths() {
    // Shard has wrong length (3 chars vs the required 2) — the
    // most common "wrong shape" failure mode for a path that
    // accidentally ends up here from outside the CAS layout.
    assert!(cas_path_digest(Path::new("/tmp/foo")).is_none());
    // Non-hex shard.
    assert!(cas_path_digest(&PathBuf::from("/tmp/zz/abc")).is_none());
    // Right shard shape but the stem is far too short to be
    // half of a sha512 digest — explicitly exercises the
    // length check so a future refactor can't silently weaken
    // it back to "any non-empty hex string".
    assert!(cas_path_digest(&PathBuf::from("/tmp/ab/cd")).is_none());
    // Stem one char short of the full 126.
    let short = format!("/tmp/ab/{}", "c".repeat(125));
    assert!(cas_path_digest(&PathBuf::from(short)).is_none());
    // Stem one char too long.
    let long = format!("/tmp/ab/{}", "c".repeat(127));
    assert!(cas_path_digest(&PathBuf::from(long)).is_none());
    // Right total length but with a non-hex byte in the stem.
    let mut bogus_stem = "c".repeat(125);
    bogus_stem.push('z');
    let bad_hex = format!("/tmp/ab/{bogus_stem}");
    assert!(cas_path_digest(&PathBuf::from(bad_hex)).is_none());
}

#[test]
fn synthesize_files_index_recovers_digest_size_and_exec_bit() {
    // The slow path computes `CafsFileInfo` by reading every file,
    // re-hashing, and stat'ing. The fast path must produce the
    // same `(digest, mode-class, size)` triple from the CAS path
    // alone — anything else and the warm prefetch would miss.
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let (regular_path, regular_hash) = store_dir.write_cas_file(b"abc", false).unwrap();
    let (exec_path, exec_hash) = store_dir.write_cas_file(b"#!/usr/bin/env node\n", true).unwrap();

    let mut cas_paths = HashMap::new();
    cas_paths.insert("README.md".to_string(), regular_path);
    cas_paths.insert("bin/run".to_string(), exec_path);

    let index = synthesize_files_index(&cas_paths).unwrap();
    assert_eq!(index.len(), 2);

    let readme = index.get("README.md").expect("README entry");
    assert_eq!(readme.digest, format!("{regular_hash:x}"));
    assert_eq!(readme.size, 3);
    assert_eq!(readme.mode & 0o111, 0, "regular files have no exec bit");
    assert_eq!(readme.checked_at, None);

    let bin = index.get("bin/run").expect("bin entry");
    assert_eq!(bin.digest, format!("{exec_hash:x}"));
    assert_eq!(bin.size, b"#!/usr/bin/env node\n".len() as u64);
    assert_eq!(bin.mode & 0o111, 0o111, "exec files keep all exec bits");
}

#[test]
fn synthesize_files_index_errors_on_malformed_cas_path() {
    // A caller handing us paths that don't match the v11 CAS
    // layout is a programming error — better to surface it as
    // `InvalidData` than to silently bake a bogus digest into
    // `index.db`.
    let mut bad = HashMap::new();
    // A path that exists but isn't shaped like a CAS file.
    let tmp = tempdir().unwrap();
    let scratch = tmp.path().join("scratch.txt");
    std::fs::write(&scratch, b"x").unwrap();
    bad.insert("scratch.txt".to_string(), scratch);

    let err = synthesize_files_index(&bad).unwrap_err();
    match err {
        GitFetcherError::Io(io_err) => assert_eq!(io_err.kind(), io::ErrorKind::InvalidData),
        other => panic!("expected Io(InvalidData), got {other:?}"),
    }
}

#[test]
fn materialize_into_rejects_traversal() {
    // The dispatcher must never write a file outside `target_dir`
    // even when handed a malicious `cas_paths` map. Build one
    // with a `..` entry and confirm we get InvalidInput.
    let target = tempdir().unwrap();
    let cas_root = tempdir().unwrap();
    let store_dir = StoreDir::from(cas_root.path().to_path_buf());
    let (cas_path, _hash) = store_dir.write_cas_file(b"poison\n", false).unwrap();

    let mut bad: HashMap<String, _> = HashMap::new();
    bad.insert("../escape".to_string(), cas_path);

    let err = materialize_into(&bad, target.path()).unwrap_err();
    assert_invalid_input(err);
    // The `escape` file must not exist anywhere — neither in the
    // target dir nor in its parent.
    assert!(!target.path().join("escape").exists());
    assert!(!target.path().parent().unwrap().join("escape").exists());
}

/// Strip the store file's exec bit first so the assertion proves restoration,
/// not `fs::copy`'s mode carry-over; the non-exec sibling pins the no-widen path.
#[cfg(unix)]
#[test]
fn materialize_into_restores_exec_bit_from_cas_suffix() {
    use std::{fs, os::unix::fs::PermissionsExt};

    let target = tempdir().unwrap();
    let cas_root = tempdir().unwrap();
    let store_dir = StoreDir::from(cas_root.path().to_path_buf());

    let (exec_cas, _) = store_dir.write_cas_file(b"#!/bin/sh\n", true).unwrap();
    fs::set_permissions(&exec_cas, fs::Permissions::from_mode(0o644)).unwrap();
    let (regular_cas, _) = store_dir.write_cas_file(b"data\n", false).unwrap();
    fs::set_permissions(&regular_cas, fs::Permissions::from_mode(0o600)).unwrap();

    let mut cas_paths: HashMap<String, _> = HashMap::new();
    cas_paths.insert("bin/run".to_string(), exec_cas);
    cas_paths.insert("README.md".to_string(), regular_cas);

    materialize_into(&cas_paths, target.path()).unwrap();

    let exec_mode = fs::metadata(target.path().join("bin/run")).unwrap().permissions().mode();
    assert_eq!(exec_mode & 0o777, 0o755, "exec-suffixed CAS file must materialize as 0o755");
    let regular_mode = fs::metadata(target.path().join("README.md")).unwrap().permissions().mode();
    assert_eq!(regular_mode & 0o777, 0o600, "non-exec file must keep its restrictive mode");
}
