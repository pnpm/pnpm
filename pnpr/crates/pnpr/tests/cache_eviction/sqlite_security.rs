//! Symlink-hardening, store-index error retention, and verdict-expiry dry-run parity tests.

use super::{create_file_with_age, test_config};
#[cfg(unix)]
use pnpm_store_dir::{CafsFileInfo, PackageFilesIndex, StoreDir, StoreIndex};
use pnpr::cache_maintenance::{CacheGcEffect, CacheGcOptions, CacheGcScope, collect_cache};
use rusqlite::Connection;
#[cfg(unix)]
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

#[cfg(unix)]
#[tokio::test]
async fn symlinked_resolver_sqlite_paths_do_not_mutate_external_dbs_and_retain_cas_files() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    std::fs::create_dir_all(cache_dir).unwrap();
    std::fs::write(
        cache_dir.join(pnpr_config::CACHE_SECRET_MARKER),
        pnpr_config::cache_secret_fingerprint(&config.cache_secret),
    )
    .unwrap();

    let outside = TempDir::new().unwrap();
    let ext_verdict_dir = outside.path().join("ext-verdicts");
    std::fs::create_dir_all(&ext_verdict_dir).unwrap();
    let ext_verdict_db = ext_verdict_dir.join("lockfile-verdicts.sqlite");
    let conn = Connection::open(&ext_verdict_db).unwrap();
    conn.execute_batch(
        "CREATE TABLE lockfile_verdicts (
             hash TEXT PRIMARY KEY,
             policy TEXT NOT NULL,
             verified_at_ms INTEGER NOT NULL
         );
         INSERT INTO lockfile_verdicts VALUES ('victim-hash', '{}', 1);",
    )
    .unwrap();
    drop(conn);

    // Case 1: `resolver-cache` itself is a symlink to an external directory.
    std::os::unix::fs::symlink(&ext_verdict_dir, cache_dir.join("resolver-cache")).unwrap();
    let _ = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: Some(0),
            max_age: Some(Duration::from_secs(1)),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    let conn = Connection::open(&ext_verdict_db).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM lockfile_verdicts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1, "symlinked resolver-cache must never mutate external verdict DB");
    drop(conn);

    // Case 2: `resolver-cache` is a real directory, `lockfile-verdicts.sqlite` is a symlink.
    std::fs::remove_file(cache_dir.join("resolver-cache")).unwrap();
    std::fs::create_dir_all(cache_dir.join("resolver-cache")).unwrap();
    std::os::unix::fs::symlink(
        &ext_verdict_db,
        cache_dir.join("resolver-cache/lockfile-verdicts.sqlite"),
    )
    .unwrap();

    // Case 3: `resolver-store/v11/index.db` is a symlink while an expired CAS file is pending.
    let ext_store = StoreDir::new(outside.path().join("ext-store"));
    let ext_index = StoreIndex::open_in(&ext_store).unwrap();
    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo {
            digest: "ab0123456789".to_string(),
            mode: 0o644,
            size: 12,
            checked_at: None,
        },
    );
    ext_index
        .set(
            "sha512-ext\text-pkg@1.0.0",
            &PackageFilesIndex {
                manifest: None,
                requires_build: None,
                requires_prepare: None,
                algo: "sha512".to_string(),
                files,
                side_effects: None,
                remote_side_effects_quarantine: None,
                tar_integrity: None,
            },
        )
        .unwrap();
    drop(ext_index);

    let local_store = StoreDir::new(cache_dir.join("resolver-store"));
    let cas_file = local_store.root().join("files/ab/0123456789");
    create_file_with_age(&cas_file, b"cas-contents", Duration::from_hours(48));
    std::os::unix::fs::symlink(
        ext_store.root().join("index.db"),
        local_store.root().join("index.db"),
    )
    .unwrap();

    let report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: Some(0),
            max_age: Some(Duration::from_hours(1)),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    assert_eq!(report.files_evicted, 0, "CAS file must be retained when store index prune fails");
    assert!(cas_file.exists(), "CAS file must remain on disk when index.db is rejected");

    let conn = Connection::open(&ext_verdict_db).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM lockfile_verdicts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1, "symlinked lockfile-verdicts.sqlite must never be mutated");

    let reopened_ext = StoreIndex::open_in(&ext_store).unwrap();
    assert!(
        reopened_ext
            .get("sha512-ext\text-pkg@1.0.0")
            .unwrap()
            .is_some(),
        "external StoreIndex targeted by a symlink must remain intact",
    );
}

#[cfg(unix)]
#[tokio::test]
async fn readonly_store_index_retains_cas_files_instead_of_orphaning_rows() {
    use std::os::unix::fs::PermissionsExt as _;

    // SAFETY: `geteuid` takes no arguments and is always safe to call.
    let euid = unsafe { libc::geteuid() };
    assert_ne!(
        euid, 0,
        "chmod 0o444 does not restrict root; run this test as an unprivileged user",
    );

    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    std::fs::create_dir_all(cache_dir).unwrap();
    std::fs::write(
        cache_dir.join(pnpr_config::CACHE_SECRET_MARKER),
        pnpr_config::cache_secret_fingerprint(&config.cache_secret),
    )
    .unwrap();

    let store_dir = StoreDir::new(cache_dir.join("resolver-store"));
    let cas_file = store_dir.root().join("files/cd/9876543210");
    create_file_with_age(&cas_file, b"precious-cas", Duration::from_hours(48));

    let index = StoreIndex::open_in(&store_dir).unwrap();
    let mut files = HashMap::new();
    files.insert(
        "index.js".to_string(),
        CafsFileInfo {
            digest: "cd9876543210".to_string(),
            mode: 0o644,
            size: 12,
            checked_at: None,
        },
    );
    index
        .set(
            "sha512-ro\tro-pkg@1.0.0",
            &PackageFilesIndex {
                manifest: None,
                requires_build: None,
                requires_prepare: None,
                algo: "sha512".to_string(),
                files,
                side_effects: None,
                remote_side_effects_quarantine: None,
                tar_integrity: None,
            },
        )
        .unwrap();
    drop(index);

    let db_path = store_dir.root().join("index.db");
    std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o444)).unwrap();

    let report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: None,
            max_age: Some(Duration::from_hours(1)),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert_eq!(report.files_evicted, 0);
    assert!(cas_file.exists(), "CAS file must not be unlinked when StoreIndex delete fails");
    let reopened = StoreIndex::open_in(&store_dir).unwrap();
    assert!(
        reopened
            .get("sha512-ro\tro-pkg@1.0.0")
            .unwrap()
            .is_some(),
    );
}

#[tokio::test]
async fn verdict_expiry_below_vacuum_threshold_matches_dry_run_and_apply_near_max_size() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    std::fs::create_dir_all(cache_dir).unwrap();
    let marker = cache_dir.join(pnpr_config::CACHE_SECRET_MARKER);
    std::fs::write(&marker, pnpr_config::cache_secret_fingerprint(&config.cache_secret)).unwrap();

    let verdict_db = cache_dir.join("resolver-cache/lockfile-verdicts.sqlite");
    std::fs::create_dir_all(verdict_db.parent().unwrap()).unwrap();
    let conn = Connection::open(&verdict_db).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE lockfile_verdicts (
             hash TEXT PRIMARY KEY,
             policy TEXT NOT NULL,
             verified_at_ms INTEGER NOT NULL
         );",
    )
    .unwrap();
    let now_ms = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let old_ms = now_ms - i64::try_from(Duration::from_hours(48).as_millis()).unwrap();
    let payload = format!(r#"{{"p":"{}"}}"#, "a".repeat(900));
    // Insert 4 expired rows (~1 page, < 16 freelist threshold so expire_verdict_rows skips VACUUM)
    // and 20 fresh rows so the DB is > EMPTY_VERDICT_DB_BYTES.
    for idx in 0..4_i64 {
        conn.execute(
            "INSERT INTO lockfile_verdicts (hash, policy, verified_at_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params![format!("exp-{idx:02}"), &payload, old_ms + idx],
        )
        .unwrap();
    }
    for idx in 0..20_i64 {
        conn.execute(
            "INSERT INTO lockfile_verdicts (hash, policy, verified_at_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params![format!("fresh-{idx:02}"), &payload, now_ms - idx],
        )
        .unwrap();
    }
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
    drop(conn);

    let active_file = cache_dir.join("resolver-cache/v11/metadata-full/candidate.json");
    create_file_with_age(&active_file, &[0x55; 2000], Duration::from_mins(5));

    let base_bytes =
        std::fs::metadata(&marker).unwrap().len() + std::fs::metadata(&verdict_db).unwrap().len();
    // Total on disk is `base_bytes + 2000`. Because `expire_verdict_rows` does not VACUUM
    // (< 16 free pages), on-disk SQLite bytes stay `base_bytes`, so `max_size = base_bytes + 500`
    // must evict `active_file` in both DryRun and Apply.
    let limit = base_bytes + 500;

    let dry_report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::DryRun,
            max_size: Some(limit),
            max_age: Some(Duration::from_hours(24)),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();

    let apply_report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: Some(limit),
            max_age: Some(Duration::from_hours(24)),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();

    assert_eq!(dry_report, apply_report);
    assert_eq!(apply_report.files_evicted, 1);
    assert!(!active_file.exists());
}
