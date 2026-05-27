use super::{VerifiedFilesCache, build_file_maps_from_index, check_pkg_files_integrity};
use crate::{CafsFileInfo, PackageFilesIndex, SideEffectsDiff, StoreDir};
use pretty_assertions::assert_eq;
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tempfile::tempdir;

/// Write `content` to the correct CAFS path under `store_dir` for
/// the given hex digest. Returns the path.
fn plant_cafs_file(store_dir: &StoreDir, digest: &str, mode: u32, content: &[u8]) -> PathBuf {
    let path = store_dir.cas_file_path_by_mode(digest, mode).expect("valid digest");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = fs::File::create(&path).unwrap();
    f.write_all(content).unwrap();
    f.sync_all().ok();
    path
}

fn sha512_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha512::digest(bytes))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

fn index_with(algo: &str, info: Vec<(&str, CafsFileInfo)>) -> PackageFilesIndex {
    PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: algo.to_string(),
        files: info.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        side_effects: None,
    }
}

fn info(digest: &str, size: u64, mode: u32, checked_at: Option<u64>) -> CafsFileInfo {
    CafsFileInfo { checked_at, digest: digest.to_string(), mode, size }
}

/// `build_file_maps_from_index` never stats the files. With a
/// valid digest, it returns a populated `files_map` with
/// `passed = true` regardless of whether anything is on disk —
/// the sibling `fast_path_fails_when_digest_is_malformed` covers
/// the "digest was not resolvable" failure case.
#[test]
fn fast_path_skips_filesystem_checks() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let digest = sha512_hex(b"dummy");
    let entry = index_with("sha512", vec![("index.js", info(&digest, 5, 0o644, None))]);
    let result = build_file_maps_from_index(&store_dir, entry);
    dbg!(&result);
    assert!(result.passed, "fast path passes for a valid digest without touching the disk");
    let path = result.files_map.get("index.js").expect("path inserted");
    eprintln!("path={path:?} exists={}", path.exists());
    assert!(!path.exists(), "no file was planted — fast path didn't care");
}

/// On-disk file is live, `checked_at` is far in the future so the
/// 100 ms slack keeps the mtime delta negative and we take the
/// "unmodified, trust the digest" branch — without any `fs::read`.
///
/// We can't easily set `mtime` from the standard library, but
/// `checked_at` in the row is caller-controlled, so setting it
/// above the real `mtime` is enough to exercise the trust path.
#[test]
fn careful_path_trusts_file_when_mtime_is_within_slack() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let content = b"hello, cafs";
    let digest = sha512_hex(content);
    let _path = plant_cafs_file(&store_dir, &digest, 0o644, content);
    let future = now_ms() + 3_600_000; // one hour from now
    let entry = index_with(
        "sha512",
        vec![("index.js", info(&digest, content.len() as u64, 0o644, Some(future)))],
    );
    let result = check_pkg_files_integrity(&store_dir, entry, &VerifiedFilesCache::new());
    dbg!(&result);
    assert!(result.passed);
    assert_eq!(result.files_map.len(), 1);
}

/// Missing on disk → whole entry fails so the caller re-fetches.
/// `files_map` is still populated for diagnostics.
#[test]
fn careful_path_fails_on_missing_cafs_file() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let digest = sha512_hex(b"nope");
    let entry = index_with("sha512", vec![("README", info(&digest, 4, 0o644, None))]);
    let result = check_pkg_files_integrity(&store_dir, entry, &VerifiedFilesCache::new());
    dbg!(&result);
    assert!(!result.passed, "missing file → fail");
    assert_eq!(result.files_map.len(), 1);
}

/// File is on disk, the row claims the digest is for *different*
/// bytes, size matches. `checked_at = None` ≡ 0, so the mtime-slack
/// delta is "definitely > 100 ms", forcing re-hash → mismatch →
/// `remove_file` + fail. Ports pnpm's `verifyFile` wrong-digest
/// branch.
#[test]
fn careful_path_removes_file_whose_content_hash_mismatches() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let fake_digest = sha512_hex(b"claimed content");
    let actual = b"actual bytes!!!";
    let path = plant_cafs_file(&store_dir, &fake_digest, 0o644, actual);
    let entry = index_with(
        "sha512",
        vec![("whatever", info(&fake_digest, actual.len() as u64, 0o644, Some(0)))],
    );
    let result = check_pkg_files_integrity(&store_dir, entry, &VerifiedFilesCache::new());
    dbg!(&result);
    assert!(!result.passed, "bad hash → fail");
    eprintln!("path={path:?} exists={}", path.exists());
    assert!(!path.exists(), "mismatched file is removed so the next call re-fetches");
}

/// Row claims size 999 but the file has 14 bytes. `checked_at = 0`
/// puts us firmly in the "modified" branch (mtime now > 100 ms past
/// 0). Size mismatch short-circuits before any re-hash. Ports
/// pnpm's `currentFile.size !== fstat.size` branch.
#[test]
fn careful_path_removes_file_whose_size_mismatches_after_touch() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let content = b"actual content";
    let digest = sha512_hex(content);
    let path = plant_cafs_file(&store_dir, &digest, 0o644, content);
    let entry = index_with("sha512", vec![("mismatch", info(&digest, 999, 0o644, Some(0)))]);
    let result = check_pkg_files_integrity(&store_dir, entry, &VerifiedFilesCache::new());
    dbg!(&result);
    assert!(!result.passed);
    eprintln!("path={path:?} exists={}", path.exists());
    assert!(!path.exists(), "size mismatch removes the file so a re-fetch starts clean");
}

/// Two filenames pointing at the same CAFS path verify once, not
/// twice. Ports the `verifiedFilesCache` behaviour.
#[test]
fn careful_path_dedups_by_digest_within_a_single_entry() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let content = b"shared blob";
    let digest = sha512_hex(content);
    let _path = plant_cafs_file(&store_dir, &digest, 0o644, content);
    let future = now_ms() + 3_600_000;
    let info_shared = info(&digest, content.len() as u64, 0o644, Some(future));
    let entry = index_with(
        "sha512",
        vec![("a.txt", info_shared.clone_for_test()), ("b.txt", info_shared.clone_for_test())],
    );
    let result = check_pkg_files_integrity(&store_dir, entry, &VerifiedFilesCache::new());
    dbg!(&result);
    assert!(result.passed);
    assert_eq!(result.files_map.len(), 2);
}

/// A CAFS path verified during one `check_pkg_files_integrity` call
/// must not be re-verified by the next call when both share the
/// same `VerifiedFilesCache`. Ports pnpm's install-scoped
/// `verifiedFilesCache: Set<string>` semantics.
///
/// The proof: plant the file, run a successful first verify against
/// it (populates the cache), then *delete* the file and run a
/// second verify. If the cache short-circuits the second call, it
/// returns `passed: true` despite the missing file — that's the
/// observable signal that the stat was skipped. Real installs
/// don't delete files mid-install, so this artificial setup is
/// purely a test handle for the dedup behaviour.
#[test]
fn careful_path_dedups_across_calls_via_shared_cache() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let content = b"shared-across-packages";
    let digest = sha512_hex(content);
    let path = plant_cafs_file(&store_dir, &digest, 0o644, content);
    let future = now_ms() + 3_600_000;
    let info_shared = info(&digest, content.len() as u64, 0o644, Some(future));

    let cache = VerifiedFilesCache::new();

    let entry_a = index_with("sha512", vec![("a-pkg/index.js", info_shared.clone_for_test())]);
    let result_a = check_pkg_files_integrity(&store_dir, entry_a, &cache);
    dbg!(&result_a);
    assert!(result_a.passed, "first call verifies the live file");
    eprintln!("cache.contains(&path)={}", cache.contains(&path));
    assert!(cache.contains(&path), "successful verify populates the shared cache");

    // Pull the rug out from under the second call. Without the
    // shared cache we'd stat-and-fail; with it, the path is
    // already in `cache` so the inner `verify_file` is skipped.
    std::fs::remove_file(&path).unwrap();
    let entry_b = index_with("sha512", vec![("b-pkg/index.js", info_shared.clone_for_test())]);
    let result_b = check_pkg_files_integrity(&store_dir, entry_b, &cache);
    dbg!(&result_b);
    assert!(
        result_b.passed,
        "second call should short-circuit via the shared cache and skip the now-missing file",
    );
}

/// Same digest with different `mode` resolves to two distinct CAFS
/// paths (`<hex>` vs `<hex>-exec`). Keying dedup by digest alone
/// would skip verifying the second path — this test plants only
/// the non-exec half and asserts the install still fails
/// verification, forcing a re-fetch, instead of returning
/// `passed: true` with a missing exec blob.
#[test]
fn careful_path_dedups_per_resolved_path_not_per_digest() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let content = b"polymode";
    let digest = sha512_hex(content);
    // Plant the non-exec variant only; leave the exec path missing.
    let non_exec_path = plant_cafs_file(&store_dir, &digest, 0o644, content);
    let exec_path = store_dir.cas_file_path_by_mode(&digest, 0o755).unwrap();
    eprintln!(
        "non_exec_path={non_exec_path:?} exec_path={exec_path:?} exec_exists={}",
        exec_path.exists(),
    );
    assert!(!exec_path.exists());
    assert_ne!(non_exec_path, exec_path);

    let future = now_ms() + 3_600_000;
    let entry = index_with(
        "sha512",
        vec![
            ("lib.js", info(&digest, content.len() as u64, 0o644, Some(future))),
            ("bin/app", info(&digest, content.len() as u64, 0o755, Some(future))),
        ],
    );
    let result = check_pkg_files_integrity(&store_dir, entry, &VerifiedFilesCache::new());
    dbg!(&result);
    assert!(
        !result.passed,
        "same digest + different mode = different CAFS path; missing exec blob must fail",
    );
}

/// Unknown algorithm in the row → treat as verification failure,
/// matching pnpm's "catch any crypto error, return false". The row
/// is on disk, the mtime delta forces re-hash, and `verify_file_integrity`
/// returns `false` because the algo isn't sha512.
#[test]
fn careful_path_fails_unknown_algo_as_verification_failure() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let content = b"bytes";
    let digest = sha512_hex(content);
    let path = plant_cafs_file(&store_dir, &digest, 0o644, content);
    let entry =
        index_with("sha256", vec![("x", info(&digest, content.len() as u64, 0o644, Some(0)))]);
    let result = check_pkg_files_integrity(&store_dir, entry, &VerifiedFilesCache::new());
    dbg!(&result);
    assert!(!result.passed);
    eprintln!("path={path:?} exists={}", path.exists());
    assert!(!path.exists(), "unknown algo → treated as corrupt → removed");
}

/// A CAFS dirent that's a directory (store corruption — stray
/// `mkdir -p` or interrupted write) must not survive verification:
/// pacquet used to reject with `remove_file(dir)` → `EISDIR`, which
/// silently failed and left the directory in place forever. The new
/// `remove_stale_cafs_entry` falls back to `remove_dir_all` so the
/// store actually self-heals on the next install.
#[test]
fn careful_path_removes_directory_at_cafs_path() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    // Plant a directory where a CAFS file belongs.
    let digest = "c".repeat(128);
    let cafs_path = store_dir.cas_file_path_by_mode(&digest, 0o644).unwrap();
    fs::create_dir_all(&cafs_path).unwrap();
    // Row claims non-zero size; `check_file` stats the dir, size
    // mismatches the row, we hit the `remove_stale_cafs_entry` path.
    let entry = index_with("sha512", vec![("impostor", info(&digest, 1_000_000, 0o644, Some(0)))]);
    let result = check_pkg_files_integrity(&store_dir, entry, &VerifiedFilesCache::new());
    dbg!(&result);
    assert!(!result.passed);
    eprintln!("cafs_path={cafs_path:?} exists={}", cafs_path.exists());
    assert!(
        !cafs_path.exists(),
        "a directory at the CAFS path must be scrubbed like a file so the next install re-fetches",
    );
}

/// `build_file_maps_from_index` shouldn't silently drop unresolvable
/// entries — that would give the caller a partial `files_map` and a
/// cache hit with missing files. Flip `passed` to `false` when any
/// digest can't be turned into a CAFS path so the caller re-fetches.
#[test]
fn fast_path_fails_when_digest_is_malformed() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let entry = index_with("sha512", vec![("bad-digest", info("not-hex", 10, 0o644, None))]);
    let result = build_file_maps_from_index(&store_dir, entry);
    dbg!(&result);
    assert!(!result.passed, "malformed digest → whole entry fails so caller re-fetches");
    assert_eq!(result.files_map.len(), 0);
}

// `CafsFileInfo` is `!Clone` in production (no need there). Give
// the tests an explicit helper so each assertion builds its own
// copy without implying a production `Clone` impl.
impl CafsFileInfo {
    fn clone_for_test(&self) -> Self {
        Self {
            checked_at: self.checked_at,
            digest: self.digest.clone(),
            mode: self.mode,
            size: self.size,
        }
    }
}

/// No `side_effects` field on the index → `VerifyResult.side_effects_maps`
/// is `None`. Distinguishes "this package never had a cache entry
/// written" from "cache configured but empty for this key" — the
/// importer treats the former as a regular non-built import.
#[test]
fn no_side_effects_yields_none() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let digest = sha512_hex(b"x");
    let entry = index_with("sha512", vec![("a", info(&digest, 1, 0o644, None))]);
    let result = build_file_maps_from_index(&store_dir, entry);
    assert!(result.side_effects_maps.is_none());
}

/// One cache key, one `added` file, one `deleted` file: the
/// overlay is `added` ∪ (base \ deleted) — entries in `added` win
/// when both layers name the same filename. Mirrors upstream's
/// [`applySideEffectsDiffWithMaps`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/store/create-cafs-store/src/index.ts#L103-L121).
#[test]
fn side_effects_overlay_adds_and_drops_correctly() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let base_digest = sha512_hex(b"base");
    let added_digest = sha512_hex(b"added");
    // base files: a.js, b.js. Side-effects for one cache key:
    // add c.js, delete b.js. Overlay should land {a.js, c.js}.
    let mut side_effects = HashMap::new();
    let mut added = HashMap::new();
    added.insert("c.js".to_string(), info(&added_digest, 5, 0o644, None));
    side_effects.insert(
        "darwin;arm64;node20;deps=fake".to_string(),
        SideEffectsDiff { added: Some(added), deleted: Some(vec!["b.js".to_string()]) },
    );
    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".into(),
        files: HashMap::from([
            ("a.js".to_string(), info(&base_digest, 4, 0o644, None)),
            ("b.js".to_string(), info(&base_digest, 4, 0o644, None)),
        ]),
        side_effects: Some(side_effects),
    };
    let result = build_file_maps_from_index(&store_dir, entry);
    let maps = result.side_effects_maps.expect("populated");
    let overlay = maps.get("darwin;arm64;node20;deps=fake").expect("entry exists");
    assert!(overlay.contains_key("a.js"), "base survives: {overlay:?}");
    assert!(overlay.contains_key("c.js"), "added overlays: {overlay:?}");
    assert!(!overlay.contains_key("b.js"), "deleted drops: {overlay:?}");
    assert_eq!(overlay.len(), 2);
}

/// `added` wins over `base` when the filenames collide. The base
/// path is shadowed by the side-effects path.
#[test]
fn side_effects_overlay_added_shadows_base_on_collision() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let base_digest = sha512_hex(b"base");
    let overlay_digest = sha512_hex(b"overlay-shadow");
    let mut added = HashMap::new();
    added.insert("collide.js".to_string(), info(&overlay_digest, 16, 0o644, None));
    let mut side_effects = HashMap::new();
    side_effects.insert("k1".to_string(), SideEffectsDiff { added: Some(added), deleted: None });
    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".into(),
        files: HashMap::from([("collide.js".to_string(), info(&base_digest, 4, 0o644, None))]),
        side_effects: Some(side_effects),
    };
    let result = build_file_maps_from_index(&store_dir, entry);
    let overlay = result.side_effects_maps.unwrap().remove("k1").unwrap();
    let path = overlay.get("collide.js").expect("collide.js present");
    // CAFS layout splits the digest as `<2-char prefix>/<rest>`, so the
    // path won't contain the digest as a single contiguous substring.
    // Verify by checking that the overlay digest's tail (post-prefix
    // hex) appears in the path, and that the base digest's tail does
    // NOT.
    let path_str = path.to_string_lossy();
    assert!(
        path_str.contains(&overlay_digest[2..]),
        "overlay digest tail should appear in CAFS path: {path:?}",
    );
    assert!(
        !path_str.contains(&base_digest[2..]),
        "base digest tail must NOT appear (shadowed): {path:?}",
    );
}

/// A malformed digest inside an `added` overlay drops the **whole**
/// cache_key entry — not just the single bad file. Mismatched
/// overlays would otherwise turn a future `is_built = true` decision
/// into a silent corruption (build skipped, required artifact
/// missing). Other cache_key entries on the same package survive.
#[test]
fn side_effects_overlay_malformed_added_digest_drops_cache_key_entry() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let base_digest = sha512_hex(b"base");
    let good_digest = sha512_hex(b"good-added");

    let mut k_bad_added = HashMap::new();
    // One good file alongside one bad one — the whole entry should
    // still drop, not just the bad file.
    k_bad_added.insert("good.js".to_string(), info(&good_digest, 4, 0o644, None));
    k_bad_added.insert("bad.js".to_string(), info("not-hex", 4, 0o644, None));

    let mut k_good_added = HashMap::new();
    k_good_added.insert("ok.js".to_string(), info(&good_digest, 4, 0o644, None));

    let mut side_effects = HashMap::new();
    side_effects
        .insert("k-bad".to_string(), SideEffectsDiff { added: Some(k_bad_added), deleted: None });
    side_effects
        .insert("k-good".to_string(), SideEffectsDiff { added: Some(k_good_added), deleted: None });

    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".into(),
        files: HashMap::from([("base.js".to_string(), info(&base_digest, 4, 0o644, None))]),
        side_effects: Some(side_effects),
    };
    let result = build_file_maps_from_index(&store_dir, entry);
    let maps = result.side_effects_maps.expect("populated");
    assert!(!maps.contains_key("k-bad"), "k-bad must drop entirely on malformed digest");
    assert!(maps.contains_key("k-good"), "k-good must survive: {maps:?}");
}

/// Multiple cache keys produce independent overlays. One entry's
/// `added` doesn't bleed into another's.
#[test]
fn side_effects_overlay_keys_are_independent() {
    let tmp = tempdir().unwrap();
    let store_dir = StoreDir::new(tmp.path());
    let base_digest = sha512_hex(b"base");
    let added_k1 = sha512_hex(b"k1-added");
    let added_k2 = sha512_hex(b"k2-added");
    let mut side_effects = HashMap::new();
    side_effects.insert(
        "k1".to_string(),
        SideEffectsDiff {
            added: Some(HashMap::from([("a.js".to_string(), info(&added_k1, 1, 0o644, None))])),
            deleted: None,
        },
    );
    side_effects.insert(
        "k2".to_string(),
        SideEffectsDiff {
            added: Some(HashMap::from([("b.js".to_string(), info(&added_k2, 1, 0o644, None))])),
            deleted: None,
        },
    );
    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".into(),
        files: HashMap::from([("base.js".to_string(), info(&base_digest, 4, 0o644, None))]),
        side_effects: Some(side_effects),
    };
    let result = build_file_maps_from_index(&store_dir, entry);
    let maps = result.side_effects_maps.unwrap();
    let k1 = maps.get("k1").unwrap();
    let k2 = maps.get("k2").unwrap();
    assert!(k1.contains_key("a.js") && !k1.contains_key("b.js"), "k1: {k1:?}");
    assert!(k2.contains_key("b.js") && !k2.contains_key("a.js"), "k2: {k2:?}");
    // Both share base.js.
    assert!(k1.contains_key("base.js"));
    assert!(k2.contains_key("base.js"));
}
