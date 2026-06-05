use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use pacquet_lockfile::LockfileResolution;
use pacquet_resolving_resolver_base::{
    ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture,
};
use serde_json::Value as JsonValue;
use tempfile::TempDir;

use super::{
    CACHE_FILE_NAME, CacheLockfile, CachePrecomputed, CacheRecord, MAX_CACHE_ENTRIES,
    record_verification, try_lockfile_verification_cache,
};

/// Trivial verifier that records what policy it advertises and
/// whether it trusts a cached policy snapshot.
struct Stub {
    policy: serde_json::Map<String, JsonValue>,
    trusts_past: bool,
}

impl Stub {
    fn new(trusts_past: bool) -> Arc<Self> {
        Arc::new(Self { policy: serde_json::Map::new(), trusts_past })
    }

    fn with_policy(trusts_past: bool, policy: serde_json::Map<String, JsonValue>) -> Arc<Self> {
        Arc::new(Self { policy, trusts_past })
    }
}

impl ResolutionVerifier for Stub {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        _ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        Box::pin(async { ResolutionVerification::Ok })
    }

    fn policy(&self) -> &serde_json::Map<String, JsonValue> {
        &self.policy
    }

    fn can_trust_past_check(&self, _cached: &serde_json::Map<String, JsonValue>) -> bool {
        self.trusts_past
    }
}

fn touch_lockfile(dir: &Path, contents: &str) -> PathBuf {
    fs::create_dir_all(dir).expect("mkdir for lockfile");
    let path = dir.join("pnpm-lock.yaml");
    fs::write(&path, contents).expect("write lockfile");
    path
}

/// Hash builder helper: deterministic per `text` so tests can pin
/// the expected hash without depending on the actual `hash_lockfile`.
fn hashed(text: &str) -> impl FnMut() -> String + use<'_> {
    move || format!("hash-of-{text}")
}

/// Cold cache (no file) → miss. The function returns hit=false
/// with a populated `stat` (the lockfile is real); `hash` may be
/// `None` since no work needs the hash yet.
#[test]
fn cold_cache_misses_with_populated_stat() {
    let dir = TempDir::new().expect("tempdir");
    let lockfile = touch_lockfile(dir.path(), "lockfileVersion: '9.0'\n");
    let result = try_lockfile_verification_cache(dir.path(), &lockfile, &[], hashed("foo"));
    assert!(!result.hit);
    assert!(result.precomputed.stat.is_some(), "stat populated on cold miss");
}

/// After a successful `record_verification`, a follow-up lookup at
/// the same path hits the stat shortcut — same size + `mtime_ns` +
/// inode → no rehash. Verifies the `byPath` index.
#[test]
fn stat_shortcut_hits_same_path_same_stat() {
    let dir = TempDir::new().expect("tempdir");
    let lockfile = touch_lockfile(dir.path(), "lockfileVersion: '9.0'\nfoo: bar\n");
    let verifiers: Vec<Arc<dyn ResolutionVerifier>> =
        vec![Stub::new(true) as Arc<dyn ResolutionVerifier>];

    record_verification(
        dir.path(),
        &lockfile,
        &verifiers,
        || "deterministic-hash".to_string(),
        CachePrecomputed::default(),
    );

    let mut calls = 0;
    let mut hash_lockfile = || {
        calls += 1;
        "should-not-be-called".to_string()
    };
    let result =
        try_lockfile_verification_cache(dir.path(), &lockfile, &verifiers, &mut hash_lockfile);
    assert!(result.hit, "same path + same stat → hit");
    assert_eq!(calls, 0, "stat shortcut skipped the hash call");
}

/// The content-hash index hits the same lockfile content at a
/// different path. The first install records under path A; the
/// second install at path B (with the same bytes) hits via hash.
#[test]
fn content_hash_lookup_finds_same_lockfile_at_different_path() {
    let dir = TempDir::new().expect("tempdir");
    let yaml = "lockfileVersion: '9.0'\nfoo: bar\n";
    let lockfile_a = touch_lockfile(&dir.path().join("worktree-a"), yaml);
    let lockfile_b = {
        let worktree = dir.path().join("worktree-b");
        fs::create_dir_all(&worktree).expect("mkdir b");
        let path = worktree.join("pnpm-lock.yaml");
        fs::write(&path, yaml).expect("write b");
        path
    };
    let verifiers: Vec<Arc<dyn ResolutionVerifier>> =
        vec![Stub::new(true) as Arc<dyn ResolutionVerifier>];

    // Record under path A with the canonical hash.
    record_verification(
        dir.path(),
        &lockfile_a,
        &verifiers,
        || "shared-hash".to_string(),
        CachePrecomputed::default(),
    );

    // Lookup at path B yields the same hash → hit via byHash.
    let result = try_lockfile_verification_cache(dir.path(), &lockfile_b, &verifiers, || {
        "shared-hash".to_string()
    });
    assert!(result.hit, "byHash hit at different path: {result:?}");
}

/// A verifier whose policy snapshot stops being trustworthy (e.g.
/// the user tightened the cutoff) invalidates the hit, even when
/// stat shortcut would otherwise have matched.
#[test]
fn policy_invalidation_misses_even_when_stat_matches() {
    let dir = TempDir::new().expect("tempdir");
    let lockfile = touch_lockfile(dir.path(), "lockfileVersion: '9.0'\n");
    let trusting_verifier: Vec<Arc<dyn ResolutionVerifier>> =
        vec![Stub::new(true) as Arc<dyn ResolutionVerifier>];
    record_verification(
        dir.path(),
        &lockfile,
        &trusting_verifier,
        || "h".to_string(),
        CachePrecomputed::default(),
    );

    // Same path + stat, but the verifier now rejects the cached
    // snapshot (tightened policy).
    let strict_verifier: Vec<Arc<dyn ResolutionVerifier>> =
        vec![Stub::new(false) as Arc<dyn ResolutionVerifier>];
    let result = try_lockfile_verification_cache(dir.path(), &lockfile, &strict_verifier, || {
        "h".to_string()
    });
    assert!(!result.hit);
}

/// `record_verification` merges every active verifier's `policy()`
/// into one bag — same-key conflicts go to the last verifier.
#[test]
fn record_verification_merges_policies() {
    let dir = TempDir::new().expect("tempdir");
    let lockfile = touch_lockfile(dir.path(), "lockfileVersion: '9.0'\n");

    let mut policy_a = serde_json::Map::new();
    policy_a.insert("minimumReleaseAge".to_string(), 60.into());
    let mut policy_b = serde_json::Map::new();
    policy_b.insert("trustPolicy".to_string(), JsonValue::String("no-downgrade".into()));
    // `minimumReleaseAge` collides — the later verifier wins.
    policy_b.insert("minimumReleaseAge".to_string(), 120.into());

    let verifiers: Vec<Arc<dyn ResolutionVerifier>> =
        vec![Stub::with_policy(true, policy_a), Stub::with_policy(true, policy_b)];

    record_verification(
        dir.path(),
        &lockfile,
        &verifiers,
        || "merged-hash".to_string(),
        CachePrecomputed::default(),
    );

    let line = fs::read_to_string(dir.path().join(CACHE_FILE_NAME)).expect("read cache");
    let record: CacheRecord = serde_json::from_str(line.trim_end()).expect("parse cache record");
    assert_eq!(record.policy.get("minimumReleaseAge").and_then(JsonValue::as_u64), Some(120));
    assert_eq!(record.policy.get("trustPolicy").and_then(JsonValue::as_str), Some("no-downgrade"));
}

/// `record_verification` is idempotent on the same `(path, hash)`
/// in the sense that every successful call appends a fresh record
/// — the byHash / byPath indexes always see the latest one on
/// read. Two distinct hashes produce two records.
#[test]
fn append_only_log_records_each_call() {
    let dir = TempDir::new().expect("tempdir");
    let lockfile = touch_lockfile(dir.path(), "lockfileVersion: '9.0'\n");
    let verifiers: Vec<Arc<dyn ResolutionVerifier>> =
        vec![Stub::new(true) as Arc<dyn ResolutionVerifier>];
    for i in 0..3 {
        record_verification(
            dir.path(),
            &lockfile,
            &verifiers,
            || format!("hash-{i}"),
            CachePrecomputed::default(),
        );
    }
    let contents = fs::read_to_string(dir.path().join(CACHE_FILE_NAME)).expect("read cache");
    assert_eq!(contents.lines().filter(|line| !line.is_empty()).count(), 3);
}

/// Compaction kicks in past the byte threshold: a poisoned log full
/// of duplicate-key records gets reduced to the latest record per
/// `(path, hash)` and trimmed to `MAX_CACHE_ENTRIES`.
#[test]
fn compaction_dedupes_by_path_and_hash() {
    let dir = TempDir::new().expect("tempdir");
    let lockfile = touch_lockfile(dir.path(), "lockfileVersion: '9.0'\n");
    let cache_path = dir.path().join(CACHE_FILE_NAME);

    // Pre-seed with a >1.5 MB file of duplicate records (same path,
    // same hash). After the next `record_verification`, compaction
    // kicks in and trims to the latest entry. Serialize each record
    // through serde so the path field round-trips correctly across
    // platforms (Windows backslashes need JSON escaping).
    let mut seed = String::with_capacity(2 * 1024 * 1024);
    let mut counter: u64 = 0;
    while (seed.len() as u64) <= super::COMPACT_TRIGGER_BYTES {
        let record = CacheRecord {
            lockfile: CacheLockfile {
                hash: "dup".into(),
                path: lockfile.to_string_lossy().into_owned(),
                size: 0,
                mtime_ns: "0".into(),
                inode: "0".into(),
            },
            verified_at: counter.to_string(),
            policy: serde_json::Map::new(),
        };
        seed.push_str(&serde_json::to_string(&record).expect("serialize record"));
        seed.push('\n');
        counter += 1;
    }
    fs::write(&cache_path, &seed).expect("seed cache");
    assert!(seed.len() as u64 > super::COMPACT_TRIGGER_BYTES, "seed must trigger compaction");

    let verifiers: Vec<Arc<dyn ResolutionVerifier>> =
        vec![Stub::new(true) as Arc<dyn ResolutionVerifier>];
    record_verification(
        dir.path(),
        &lockfile,
        &verifiers,
        || "new-hash".to_string(),
        CachePrecomputed::default(),
    );

    let contents = fs::read_to_string(&cache_path).expect("read post-compact");
    let lines: Vec<&str> = contents.lines().filter(|line| !line.is_empty()).collect();
    assert!(lines.len() <= MAX_CACHE_ENTRIES + 1, "trimmed past cap: {}", lines.len());
    // Original duplicates collapsed to one (path, hash="dup"), plus
    // the freshly-recorded line with hash="new-hash".
    assert!(lines.len() <= 2, "duplicates collapsed: got {} lines", lines.len());
}

/// Malformed JSONL lines are skipped without failing the lookup
/// (other lines still parse). Mirrors upstream's "skip; the next
/// clean append will still work" tolerance.
#[test]
fn malformed_lines_are_tolerated_on_read() {
    let dir = TempDir::new().expect("tempdir");
    let lockfile = touch_lockfile(dir.path(), "lockfileVersion: '9.0'\n");
    // Build the record via the typed shape so the path encodes as
    // a JSON string with proper escaping (matters on Windows where
    // the lockfile path contains backslashes).
    let record = CacheRecord {
        lockfile: CacheLockfile {
            hash: "H".into(),
            path: lockfile.to_string_lossy().into_owned(),
            size: 0,
            mtime_ns: "0".into(),
            inode: "0".into(),
        },
        verified_at: "now".into(),
        policy: serde_json::Map::new(),
    };
    let good = serde_json::to_string(&record).expect("serialize record");
    let cache_path = dir.path().join(CACHE_FILE_NAME);
    let contents = format!("garbage line\n{good}\nmore garbage\n");
    fs::write(&cache_path, contents).expect("seed");

    // We didn't write the actual stat values, so the stat shortcut
    // misses; the content-hash route needs the verifier to trust
    // the recorded policy.
    let verifiers: Vec<Arc<dyn ResolutionVerifier>> =
        vec![Stub::new(true) as Arc<dyn ResolutionVerifier>];
    let result =
        try_lockfile_verification_cache(dir.path(), &lockfile, &verifiers, || "H".to_string());
    assert!(result.hit, "good record found despite surrounding garbage");
}

/// `CacheLockfile` round-trips through serde with the camelCase
/// field names upstream uses, so pacquet writes records pnpm can
/// read (and vice versa).
#[test]
fn cache_lockfile_serializes_with_camelcase_fields() {
    let record = CacheRecord {
        lockfile: CacheLockfile {
            hash: "h".into(),
            path: "/p".into(),
            size: 42,
            mtime_ns: "100".into(),
            inode: "5".into(),
        },
        verified_at: "now".into(),
        policy: serde_json::Map::new(),
    };
    let json = serde_json::to_string(&record).expect("serialize");
    assert!(json.contains(r#""mtimeNs":"100""#), "got: {json}");
    assert!(json.contains(r#""verifiedAt":"now""#), "got: {json}");
}
