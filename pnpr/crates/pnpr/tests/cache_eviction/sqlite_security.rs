//! Symlink-hardening, store-index error retention, and verdict-expiry dry-run parity tests.

use super::{create_file_with_age, test_config};
#[cfg(unix)]
use pnpm_store_dir::{CafsFileInfo, PackageFilesIndex, StoreDir, StoreIndex, StoreIndexFormat};
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

#[cfg(unix)]
#[tokio::test]
async fn symlinked_files_index_shard_is_rejected_and_lru_continues_past_skipped_cas() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    std::fs::create_dir_all(cache_dir).unwrap();
    let marker = cache_dir.join(pnpr_config::CACHE_SECRET_MARKER);
    std::fs::write(&marker, pnpr_config::cache_secret_fingerprint(&config.cache_secret)).unwrap();

    // Part 1: `files`-format StoreIndex with a symlinked shard must never delete outside entries.
    let outside = TempDir::new().unwrap();
    let ext_store = StoreDir::new(outside.path().join("ext-files-store"));
    std::fs::create_dir_all(ext_store.root()).unwrap();
    StoreIndex::apply_format(ext_store.root(), StoreIndexFormat::Files).unwrap();
    let ext_index = StoreIndex::open_in(&ext_store).unwrap();
    let mut files = HashMap::new();
    files.insert(
        "index.js".to_string(),
        CafsFileInfo {
            digest: "ef0123456789".to_string(),
            mode: 0o644,
            size: 12,
            checked_at: None,
        },
    );
    let pkg_key = "sha512-files\tfiles-pkg@1.0.0";
    ext_index
        .set(
            pkg_key,
            &PackageFilesIndex {
                manifest: None,
                requires_build: None,
                requires_prepare: None,
                algo: "sha512".to_string(),
                files: files.clone(),
                side_effects: None,
                remote_side_effects_quarantine: None,
                tar_integrity: None,
            },
        )
        .unwrap();
    drop(ext_index);

    let local_store = StoreDir::new(cache_dir.join("resolver-store").join("v11"));
    assert!(local_store.root().ends_with("resolver-store/v11"));
    std::fs::create_dir_all(local_store.root().join("index")).unwrap();
    let cas_file = local_store.root().join("files/ef/0123456789");
    create_file_with_age(&cas_file, &[0x41; 4000], Duration::from_hours(48));
    for entry in std::fs::read_dir(ext_store.root().join("index")).unwrap() {
        let entry = entry.unwrap();
        std::os::unix::fs::symlink(
            entry.path(),
            local_store
                .root()
                .join("index")
                .join(entry.file_name()),
        )
        .unwrap();
    }

    // Also add a newer non-CAS candidate so if CAS eviction fails under `max_size`,
    // `enforce_max_size` must continue past the retained CAS file and evict `fallback_file`!
    let fallback_file = cache_dir.join("resolver-cache/v11/metadata-full/fallback.json");
    create_file_with_age(&fallback_file, &[0x42; 3000], Duration::from_mins(10));

    let limit = std::fs::metadata(&marker).unwrap().len() + 4500;
    let report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: Some(limit),
            max_age: None,
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();

    assert!(cas_file.exists(), "CAS file must be retained when files-index shard is a symlink");
    assert!(
        !fallback_file.exists(),
        "enforce_max_size must continue evicting subsequent non-CAS entries after CAS skip",
    );
    assert_eq!(report.files_evicted, 1);
    let reopened_ext = StoreIndex::open_in(&ext_store).unwrap();
    assert!(
        reopened_ext
            .get(pkg_key)
            .unwrap()
            .is_some(),
        "external files-format StoreIndex entry must remain untouched",
    );
}

#[tokio::test]
async fn uncheckpointed_live_wal_pages_still_trim_verdict_rows_to_max_size() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    std::fs::create_dir_all(cache_dir).unwrap();
    let marker = cache_dir.join(pnpr_config::CACHE_SECRET_MARKER);
    std::fs::write(&marker, pnpr_config::cache_secret_fingerprint(&config.cache_secret)).unwrap();

    let verdict_db = cache_dir.join("resolver-cache/lockfile-verdicts.sqlite");
    let verdict_wal = cache_dir.join("resolver-cache/lockfile-verdicts.sqlite-wal");
    std::fs::create_dir_all(verdict_db.parent().unwrap()).unwrap();

    // Keep an idle connection open so SQLite leaves live pages in `-wal` instead of checkpointing on close.
    let mut holder = Connection::open(&verdict_db).unwrap();
    holder
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA wal_autocheckpoint = 0;
             CREATE TABLE lockfile_verdicts (
                 hash TEXT PRIMARY KEY,
                 policy TEXT NOT NULL,
                 verified_at_ms INTEGER NOT NULL
             );
             PRAGMA wal_checkpoint(TRUNCATE);",
        )
        .unwrap();

    let now_ms = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let payload = format!(r#"{{"p":"{}"}}"#, "w".repeat(900));
    let tx = holder.transaction().unwrap();
    for idx in 0..60_i64 {
        tx.execute(
            "INSERT INTO lockfile_verdicts (hash, policy, verified_at_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params![format!("wal-live-{idx:02}"), &payload, now_ms - idx],
        )
        .unwrap();
    }
    tx.commit().unwrap();

    let wal_len = std::fs::metadata(&verdict_wal).unwrap().len();
    assert!(wal_len > 48 * 1024, "expected live pages in -wal, got {wal_len} bytes");
    let db_len = std::fs::metadata(&verdict_db).unwrap().len();
    let shm_len = std::fs::metadata(cache_dir.join("resolver-cache/lockfile-verdicts.sqlite-shm"))
        .map(|meta| meta.len())
        .unwrap_or(0);
    let marker_len = std::fs::metadata(&marker).unwrap().len();
    let total_before = marker_len + db_len + wal_len + shm_len;
    let limit = total_before - wal_len / 2;
    assert!(total_before - limit < wal_len);

    let dry_report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::DryRun,
            max_size: Some(limit),
            max_age: None,
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    assert!(dry_report.bytes_reclaimed >= total_before - limit);

    let apply_report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: Some(limit),
            max_age: None,
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    assert!(apply_report.bytes_reclaimed > 0);

    let remaining_rows: i64 = holder
        .query_row("SELECT COUNT(*) FROM lockfile_verdicts", [], |row| row.get(0))
        .unwrap();
    assert!(remaining_rows < 60, "expected verdict rows to be trimmed, got {remaining_rows}");
    let live_total = marker_len
        + std::fs::metadata(&verdict_db).unwrap().len()
        + std::fs::metadata(&verdict_wal).map(|meta| meta.len()).unwrap_or(0)
        + std::fs::metadata(cache_dir.join("resolver-cache/lockfile-verdicts.sqlite-shm"))
            .map(|meta| meta.len())
            .unwrap_or(0);
    assert!(live_total <= limit, "live_total ({live_total}) must be <= limit ({limit})");
    drop(holder);
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_verdict_sidecar_still_counts_db_bytes_and_retries_deferred_vacuum() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    std::fs::create_dir_all(cache_dir).unwrap();
    let marker = cache_dir.join(pnpr_config::CACHE_SECRET_MARKER);
    std::fs::write(&marker, pnpr_config::cache_secret_fingerprint(&config.cache_secret)).unwrap();

    let verdict_db = cache_dir.join("resolver-cache/lockfile-verdicts.sqlite");
    std::fs::create_dir_all(verdict_db.parent().unwrap()).unwrap();
    let mut conn = Connection::open(&verdict_db).unwrap();
    conn.execute_batch(
        "CREATE TABLE lockfile_verdicts (
             hash TEXT PRIMARY KEY,
             policy TEXT NOT NULL,
             verified_at_ms INTEGER NOT NULL
         );",
    )
    .unwrap();
    let payload = format!(r#"{{"p":"{}"}}"#, "v".repeat(900));
    let tx = conn.transaction().unwrap();
    for idx in 0..80_i64 {
        tx.execute(
            "INSERT INTO lockfile_verdicts (hash, policy, verified_at_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params![format!("sidecar-{idx:02}"), &payload, 1000 + idx],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    // Leave >= 16 pages on the freelist without running VACUUM yet.
    conn.execute("DELETE FROM lockfile_verdicts WHERE verified_at_ms < 1075", [])
        .unwrap();
    drop(conn);
    let bloated_db_len = std::fs::metadata(&verdict_db).unwrap().len();
    assert!(bloated_db_len > 64 * 1024);

    // 1. A symlinked `-journal` sidecar blocks SQLite open, yet `verdict_db` bytes still count
    //    toward `max_size`, forcing `candidate` out!
    let outside = TempDir::new().unwrap();
    let outside_journal = outside.path().join("outside-journal");
    std::fs::write(&outside_journal, b"keep").unwrap();
    let journal_link = cache_dir.join("resolver-cache/lockfile-verdicts.sqlite-journal");
    std::os::unix::fs::symlink(&outside_journal, &journal_link).unwrap();

    let candidate = cache_dir.join("resolver-cache/v11/metadata-full/lru.json");
    create_file_with_age(&candidate, &[0x33; 4096], Duration::from_mins(5));
    let marker_len = std::fs::metadata(&marker).unwrap().len();
    let report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: Some(marker_len + bloated_db_len),
            max_age: None,
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    assert_eq!(report.files_evicted, 1, "verdict_db bytes must not be counted as zero");
    assert!(!candidate.exists());

    // 2. Remove the symlinked sidecar and run an expiry pass that deletes 0 rows (`max_age` cutoff
    //    predates remaining rows): it must still retry VACUUM on the >=16 freelist pages!
    std::fs::remove_file(&journal_link).unwrap();
    let vacuum_report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: None,
            max_age: Some(Duration::from_hours(24 * 365 * 80)),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    assert!(vacuum_report.bytes_reclaimed > 0, "expiry pass with 0 deleted rows must retry VACUUM");
    assert!(std::fs::metadata(&verdict_db).unwrap().len() < bloated_db_len);
}

#[tokio::test]
async fn batch_eviction_across_hundreds_of_parent_dirs_removes_every_expired_entry() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    std::fs::create_dir_all(cache_dir).unwrap();
    std::fs::write(
        cache_dir.join(pnpr_config::CACHE_SECRET_MARKER),
        pnpr_config::cache_secret_fingerprint(&config.cache_secret),
    )
    .unwrap();

    for idx in 0..300_u32 {
        let path = cache_dir.join(format!("~public/npm/packages/pkg-{idx:04}/document.json"));
        create_file_with_age(&path, b"{}", Duration::from_hours(48));
    }

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
    assert_eq!(report.files_evicted, 300);
    assert!(!cache_dir.join("~public/npm/packages").exists());
}

#[tokio::test]
async fn eviction_preserves_files_index_dir_and_skips_unreadable_subdirs() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    std::fs::create_dir_all(cache_dir).unwrap();
    std::fs::write(
        cache_dir.join(pnpr_config::CACHE_SECRET_MARKER),
        pnpr_config::cache_secret_fingerprint(&config.cache_secret),
    )
    .unwrap();

    let files_index_dir = cache_dir
        .join("resolver-store")
        .join(pnpm_store_dir::STORE_VERSION)
        .join("index");
    let shard_dir = files_index_dir.join("ab");
    std::fs::create_dir_all(&shard_dir).unwrap();
    let expired = cache_dir.join("~public/npm/packages/pkg/document.json");
    create_file_with_age(&expired, b"{}", Duration::from_hours(48));

    #[cfg(unix)]
    let locked_dir = {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = cache_dir.join("~public/npm/packages/locked");
        create_file_with_age(&dir.join("document.json"), b"{}", Duration::from_hours(48));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
        dir
    };

    let result = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: None,
            max_age: Some(Duration::from_hours(1)),
            scope: CacheGcScope::Full,
        },
    )
    .await;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o755));
    }

    let report = result.unwrap();
    assert!(report.files_evicted >= 1);
    assert!(!expired.exists(), "sibling expired entries must be evicted even if one subdir fails");
    assert!(
        files_index_dir.is_dir(),
        "empty resolver-store/<version>/index must stay intact so FilesIndex backend is preserved",
    );
}

fn backdate_shard_files(index_dir: &std::path::Path, age: Duration) {
    let past = SystemTime::now() - age;
    let times = std::fs::FileTimes::new().set_accessed(past).set_modified(past);
    for shard in std::fs::read_dir(index_dir).unwrap() {
        let shard_path = shard.unwrap().path();
        if !shard_path.is_dir() {
            continue;
        }
        backdate_files_in_dir(&shard_path, times);
    }
}

fn backdate_files_in_dir(dir: &std::path::Path, times: std::fs::FileTimes) {
    for row in std::fs::read_dir(dir).unwrap() {
        let row_path = row.unwrap().path();
        if !row_path.is_file() {
            continue;
        }
        std::fs::OpenOptions::new()
            .write(true)
            .open(row_path)
            .unwrap()
            .set_times(times)
            .unwrap();
    }
}

#[tokio::test]
async fn files_index_rows_stay_paired_with_cas() {
    use pnpm_store_dir::{CafsFileInfo, PackageFilesIndex, StoreDir, StoreIndex, StoreIndexFormat};
    use std::collections::HashMap;

    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    std::fs::create_dir_all(cache_dir).unwrap();
    let store = StoreDir::new(cache_dir.join("resolver-store").join(pnpm_store_dir::STORE_VERSION));
    std::fs::create_dir_all(store.root()).unwrap();
    StoreIndex::apply_format(store.root(), StoreIndexFormat::Files).unwrap();
    let cas_file = store.root().join("files/cd/0123456789");
    create_file_with_age(&cas_file, b"export default 42;\n", Duration::from_secs(0));
    let mut files = HashMap::new();
    files.insert(
        "index.js".to_string(),
        CafsFileInfo {
            digest: "cd0123456789".to_string(),
            mode: 0o644,
            size: 19,
            checked_at: None,
        },
    );
    let pkg_key = "sha512-files\tfiles-pkg@1.0.0";
    let index = StoreIndex::open_in(&store).unwrap();
    index
        .set(
            pkg_key,
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

    backdate_shard_files(&store.root().join("index"), Duration::from_hours(48));
    let ttl_report = collect_cache(
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
    assert_eq!(ttl_report.files_evicted, 0);
    assert!(
        StoreIndex::open_in(&store)
            .unwrap()
            .get(pkg_key)
            .unwrap()
            .is_some(),
    );
}
