use std::cell::Cell;

use tempfile::TempDir;

use super::{Map, Value, VerdictCache};

fn open() -> (TempDir, VerdictCache) {
    let dir = TempDir::new().expect("tempdir");
    let cache = VerdictCache::open(&dir.path().join("verdicts.sqlite")).expect("open cache");
    (dir, cache)
}

fn policy(min_age: i64) -> Map<String, Value> {
    let mut policy = Map::new();
    policy.insert("minimumReleaseAge".to_string(), Value::from(min_age));
    policy
}

#[test]
fn recorded_pass_is_a_hit_when_policy_trusted() {
    let (_dir, cache) = open();
    cache.record("hash-a", &policy(1440));
    assert!(cache.is_verified("hash-a", |_| true));
}

#[test]
fn absent_hash_is_a_miss() {
    let (_dir, cache) = open();
    assert!(!cache.is_verified("never-recorded", |_| true));
}

#[test]
fn untrusted_cached_policy_is_a_miss() {
    // A tightened policy: `can_trust_past_check` would return false, so
    // the cached pass must not short-circuit.
    let (_dir, cache) = open();
    cache.record("hash-a", &policy(1440));
    assert!(!cache.is_verified("hash-a", |_| false));
}

#[test]
fn hit_hands_the_cached_policy_to_the_trust_check() {
    let (_dir, cache) = open();
    cache.record("hash-a", &policy(1440));
    let saw_policy = Cell::new(false);
    let trusted = cache.is_verified("hash-a", |cached| {
        saw_policy.set(cached.get("minimumReleaseAge") == Some(&Value::from(1440)));
        true
    });
    assert!(trusted);
    assert!(saw_policy.get(), "the stored policy snapshot is what the trust check receives");
}

#[test]
fn a_corrupt_policy_row_is_evicted_on_lookup() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("verdicts.sqlite");
    let cache = VerdictCache::open(&path).expect("open cache");

    // Write a row whose policy blob isn't valid JSON, behind the cache's back.
    {
        let conn = rusqlite::Connection::open(&path).expect("open raw conn");
        conn.execute(
            "INSERT INTO lockfile_verdicts (hash, policy, verified_at_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params!["corrupt", "not json", 0_i64],
        )
        .expect("insert corrupt row");
    }

    assert!(!cache.is_verified("corrupt", |_| true), "an unparsable policy is a miss");

    let conn = rusqlite::Connection::open(&path).expect("open raw conn");
    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM lockfile_verdicts WHERE hash = ?1",
            rusqlite::params!["corrupt"],
            |row| row.get(0),
        )
        .expect("count rows");
    assert_eq!(remaining, 0, "the corrupt row self-heals (is deleted) so it can be re-recorded");
}

#[test]
fn re_recording_the_same_hash_overwrites_the_policy() {
    let (_dir, cache) = open();
    cache.record("hash-a", &policy(1440));
    cache.record("hash-a", &policy(60));
    let hit = cache.is_verified("hash-a", |cached| {
        assert_eq!(cached.get("minimumReleaseAge"), Some(&Value::from(60)));
        true
    });
    assert!(hit, "the row must still be a hit so the overwrite assertion actually runs");
}
