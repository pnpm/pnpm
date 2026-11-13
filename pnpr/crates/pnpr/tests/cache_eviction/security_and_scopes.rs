//! Private-scope secret rotation, symlink hardening, dry-run parity, and WAL verdict trim tests.

use super::{create_file_with_age, test_config};
use pnpm_store_dir::{CafsFileInfo, PackageFilesIndex, SideEffectsDiff, StoreDir, StoreIndex};
use pnpr::{
    Config,
    cache_maintenance::{
        CacheGcEffect, CacheGcOptions, CacheGcScope, active_cache_namespaces, collect_cache,
    },
};
use rusqlite::Connection;
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::TempDir;

#[tokio::test]
async fn rotating_secret_prunes_private_resolver_scopes_and_updates_marker() {
    let tmp = TempDir::new().unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 7677));
    let storage = tmp.path().join("storage");
    let cache = tmp.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();

    let old_cfg_path = tmp.path().join("old.yaml");
    std::fs::write(
        &old_cfg_path,
        format!(
            "storage: {}\ncache: {}\nsecret: old-secret-32-bytes-long-0000001\n",
            storage.display(),
            cache.display(),
        ),
    )
    .unwrap();
    let old_config = pnpr_fixtures::load_seeded_config(&old_cfg_path, listen, None);
    std::fs::write(
        cache.join(pnpr_config::CACHE_SECRET_MARKER),
        pnpr_config::cache_secret_fingerprint(&old_config.cache_secret),
    )
    .unwrap();

    let npm_priv = cache.join("resolver-cache/v12/metadata-private/old-desc/pkg.jsonl");
    let cargo_priv = cache.join("resolver-cache/cargo-index/reg1/old-desc/se/rd/serde");
    let cargo_pub = cache.join("resolver-cache/cargo-index/reg1/public/se/rd/serde");
    let py_priv = cache.join("resolver-cache/python-index/idx1/old-desc/pkg.json");
    let py_pub = cache.join("resolver-cache/python-index/idx1/public/pkg.json");
    for path in [&npm_priv, &cargo_priv, &cargo_pub, &py_priv, &py_pub] {
        create_file_with_age(path, b"{}", Duration::from_secs(10));
    }

    let new_cfg_path = tmp.path().join("new.yaml");
    std::fs::write(
        &new_cfg_path,
        format!(
            "storage: {}\ncache: {}\nsecret: new-secret-32-bytes-long-0000002\n",
            storage.display(),
            cache.display(),
        ),
    )
    .unwrap();
    let new_config = pnpr_fixtures::load_seeded_config(&new_cfg_path, listen, None);

    let dry_report = collect_cache(
        &new_config,
        &CacheGcOptions {
            effect: CacheGcEffect::DryRun,
            max_size: None,
            max_age: None,
            scope: CacheGcScope::OrphansOnly,
        },
    )
    .await
    .unwrap();
    assert_eq!(dry_report.namespaces_removed, 3);
    assert!(npm_priv.exists(), "dry-run must keep files and old marker intact");

    let apply_report = collect_cache(
        &new_config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: None,
            max_age: None,
            scope: CacheGcScope::OrphansOnly,
        },
    )
    .await
    .unwrap();
    assert_eq!(apply_report.namespaces_removed, 3);
    assert!(!npm_priv.exists());
    assert!(!cargo_priv.exists());
    assert!(!py_priv.exists());
    assert!(cargo_pub.exists(), "public cargo index stays intact across secret rotation");
    assert!(py_pub.exists(), "public python index stays intact across secret rotation");
    let stored_marker =
        std::fs::read_to_string(cache.join(pnpr_config::CACHE_SECRET_MARKER)).unwrap();
    assert_eq!(
        stored_marker.trim(),
        pnpr_config::cache_secret_fingerprint(&new_config.cache_secret),
    );
}

#[tokio::test]
async fn verdict_db_trim_does_not_wipe_all_rows_under_concurrent_wal_reader_and_dry_run_makes_no_shm()
 {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    let verdict_db = cache_dir.join("resolver-cache/lockfile-verdicts.sqlite");
    std::fs::create_dir_all(verdict_db.parent().unwrap()).unwrap();

    let writer = Connection::open(&verdict_db).unwrap();
    writer
        .execute_batch(
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
    let big_policy = format!(r#"{{"note":"{}"}}"#, "x".repeat(512));
    for idx in 0..40_i64 {
        writer
            .execute(
                "INSERT INTO lockfile_verdicts (hash, policy, verified_at_ms) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    format!("hash-{idx:04}"),
                    &big_policy,
                    now_ms - (40 - idx) * 1000
                ],
            )
            .unwrap();
    }
    writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
    drop(writer);

    let shm_path = cache_dir.join("resolver-cache/lockfile-verdicts.sqlite-shm");
    assert!(!shm_path.exists());
    let db_len = std::fs::metadata(&verdict_db).unwrap().len();
    assert!(db_len > 20 * 1024);

    let _dry = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::DryRun,
            max_size: Some(db_len - 4096),
            max_age: Some(Duration::from_secs(20)),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    assert!(!shm_path.exists(), "dry-run must not create a -shm sidecar on a clean WAL DB");

    let reader = Connection::open(&verdict_db).unwrap();
    let read_tx = reader.unchecked_transaction().unwrap();
    let initial_count: i64 = read_tx
        .query_row("SELECT COUNT(*) FROM lockfile_verdicts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(initial_count, 40);

    let shm_len = std::fs::metadata(&shm_path).unwrap().len();
    let _apply = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: Some(db_len + shm_len - 1024),
            max_age: None,
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    read_tx.finish().unwrap();
    drop(reader);

    let verify_conn = Connection::open(&verdict_db).unwrap();
    let remaining: i64 = verify_conn
        .query_row("SELECT COUNT(*) FROM lockfile_verdicts", [], |row| row.get(0))
        .unwrap();
    assert!(
        remaining > 10 && remaining < 40,
        "expected partial single-pass trim under concurrent WAL reader, got {remaining} rows",
    );
}

#[tokio::test]
async fn cli_cache_gc_rejects_unconfigured_secret_on_populated_persistent_cache() {
    let tmp = TempDir::new().unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 7677));
    let mut config = Config::from_default_yaml(tmp.path(), listen, None);
    config.storage.hosted_dir = tmp.path().join("storage");
    config.storage.cache_dir = tmp.path().join("cache");
    config.storage.metadata = pnpr_config::MetadataBackend::Sqlite(
        pnpr_config::default_metadata_path(&config.storage.hosted_dir),
    );

    let active_file = config.storage.cache_dir.join("~upstreams/existing-secret/pkg/doc.json");
    create_file_with_age(&active_file, b"{}", Duration::from_secs(10));

    let err = pnpr::run_command(
        &config,
        pnpr::Command::CacheGc {
            dry_run: false,
            max_size: None,
            max_age: None,
            orphans_only: true,
        },
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("`secret` or `secret.cache` must be configured"),
        "unexpected error: {err}",
    );
    assert!(active_file.exists(), "populated private cache must not be wiped on missing secret");
}

#[tokio::test]
async fn dry_run_matches_apply_with_orphans_staging_and_max_size() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    let active_ns = active_cache_namespaces(&config)
        .into_iter()
        .find(|ns| ns.starts_with("~public"))
        .unwrap();

    let orphan = cache_dir.join("~upstreams/orphan-ns/packages/pkg/document.json");
    create_file_with_age(&orphan, &[0x11; 1000], Duration::from_secs(30));

    let staging_base = pnpr_storage::cache_staging_tmp_base(cache_dir);
    let stale_staged = pnpr_storage::unique_tmp_path(&staging_base);
    create_file_with_age(&stale_staged, &[0x22; 500], Duration::from_mins(2));
    let fresh_staged = pnpr_storage::unique_tmp_path(&staging_base);
    create_file_with_age(&fresh_staged, &[0x33; 200], Duration::from_secs(5));

    let older_active = cache_dir.join(&active_ns).join("packages/a/blob.tgz");
    create_file_with_age(&older_active, &[0x44; 600], Duration::from_mins(5));
    let newer_active = cache_dir.join(&active_ns).join("packages/b/blob.tgz");
    create_file_with_age(&newer_active, &[0x55; 600], Duration::from_mins(1));

    let dry_report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::DryRun,
            max_size: Some(1000),
            max_age: None,
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();

    let apply_report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: Some(1000),
            max_age: None,
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();

    assert_eq!(dry_report, apply_report, "dry-run must report exact Apply totals");
    assert_eq!(apply_report.namespaces_removed, 1);
    assert_eq!(apply_report.files_evicted, 3);
    assert_eq!(apply_report.bytes_reclaimed, 2100);
    assert!(!orphan.exists());
    assert!(!stale_staged.exists());
    assert!(fresh_staged.exists());
    assert!(!older_active.exists());
    assert!(newer_active.exists());
}

#[tokio::test]
async fn store_index_prunes_rows_referencing_evicted_side_effects_blobs() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    let store_dir = StoreDir::new(cache_dir.join("resolver-store"));

    let base_cas = store_dir.root().join("files/aa/1111111111");
    create_file_with_age(&base_cas, b"base-file", Duration::from_secs(10));
    let side_effect_cas = store_dir.root().join("files/bb/2222222222-exec");
    create_file_with_age(&side_effect_cas, b"native-addon", Duration::from_hours(48));

    let mut base_files = HashMap::new();
    base_files.insert(
        "package.json".to_string(),
        CafsFileInfo { digest: "aa1111111111".to_string(), mode: 0o644, size: 9, checked_at: None },
    );
    let mut added = HashMap::new();
    added.insert(
        "build/Release/addon.node".to_string(),
        CafsFileInfo {
            digest: "bb2222222222".to_string(),
            mode: 0o755,
            size: 12,
            checked_at: None,
        },
    );
    let mut side_effects = HashMap::new();
    side_effects.insert(
        "linux-x64-node-22".to_string(),
        SideEffectsDiff { deleted: None, added: Some(added), remote_origin: None },
    );

    let store_index = StoreIndex::open_in(&store_dir).unwrap();
    store_index
        .set(
            "sha512-native\tnative-pkg@1.0.0",
            &PackageFilesIndex {
                manifest: None,
                requires_build: Some(true),
                requires_prepare: None,
                algo: "sha512".to_string(),
                files: base_files,
                side_effects: Some(side_effects),
                remote_side_effects_quarantine: None,
                tar_integrity: None,
            },
        )
        .unwrap();
    drop(store_index);

    let report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: None,
            max_age: Some(Duration::from_hours(24)),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    assert_eq!(report.files_evicted, 1);
    assert!(base_cas.exists());
    assert!(!side_effect_cas.exists());

    let reopened = StoreIndex::open_in(&store_dir).unwrap();
    assert!(
        reopened
            .get("sha512-native\tnative-pkg@1.0.0")
            .unwrap()
            .is_none(),
        "StoreIndex row must be pruned when a side_effects[*].added blob is evicted",
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_in_cache_is_neither_traversed_nor_followed_on_eviction() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;

    let outside = TempDir::new().unwrap();
    let outside_dir = outside.path().join("victim-dir");
    let outside_file = outside_dir.join("precious.txt");
    create_file_with_age(&outside_file, b"must-survive", Duration::from_hours(100));
    let outside_leaf = outside.path().join("leaf-victim.txt");
    create_file_with_age(&outside_leaf, b"leaf-survives", Duration::from_hours(100));

    let active_ns = active_cache_namespaces(&config)
        .into_iter()
        .next()
        .unwrap();
    let ns_dir = cache_dir.join(&active_ns);
    std::fs::create_dir_all(&ns_dir).unwrap();
    std::os::unix::fs::symlink(&outside_dir, ns_dir.join("dir-link")).unwrap();
    std::os::unix::fs::symlink(&outside_leaf, ns_dir.join("file-link")).unwrap();

    let report = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: Some(0),
            max_age: Some(Duration::ZERO),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    assert_eq!(report.files_evicted, 0);
    assert!(outside_file.exists(), "eviction must never traverse symlinked directories");
    assert!(outside_leaf.exists(), "eviction must never follow symlinked files");
}

#[cfg(unix)]
#[tokio::test]
async fn failed_private_scope_cleanup_preserves_old_secret_marker_for_retry() {
    use std::os::unix::fs::PermissionsExt as _;

    // SAFETY: `geteuid` takes no arguments and is always safe to call.
    let euid = unsafe { libc::geteuid() };
    assert_ne!(
        euid, 0,
        "chmod 0o555 does not restrict root; run this test as an unprivileged user",
    );

    let tmp = TempDir::new().unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 7677));
    let storage = tmp.path().join("storage");
    let cache = tmp.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();

    let old_fp = pnpr_config::cache_secret_fingerprint(b"old-secret-32-bytes-long-0000001");
    std::fs::write(cache.join(pnpr_config::CACHE_SECRET_MARKER), &old_fp).unwrap();

    let npm_priv_dir = cache.join("resolver-cache/v12/metadata-private/old-scope");
    let npm_priv_file = npm_priv_dir.join("pkg.jsonl");
    create_file_with_age(&npm_priv_file, b"{}", Duration::from_secs(10));

    let new_cfg_path = tmp.path().join("new.yaml");
    std::fs::write(
        &new_cfg_path,
        format!(
            "storage: {}\ncache: {}\nsecret: new-secret-32-bytes-long-0000002\n",
            storage.display(),
            cache.display(),
        ),
    )
    .unwrap();
    let new_config = pnpr_fixtures::load_seeded_config(&new_cfg_path, listen, None);

    std::fs::set_permissions(&npm_priv_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let first = collect_cache(
        &new_config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: None,
            max_age: None,
            scope: CacheGcScope::OrphansOnly,
        },
    )
    .await
    .unwrap();
    std::fs::set_permissions(&npm_priv_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(first.namespaces_removed, 0);
    assert!(npm_priv_file.exists());
    let marker_after_fail =
        std::fs::read_to_string(cache.join(pnpr_config::CACHE_SECRET_MARKER)).unwrap();
    assert_eq!(
        marker_after_fail.trim(),
        old_fp,
        "failed private-scope cleanup must keep old .cache-secret so the next pass retries",
    );

    let retry = collect_cache(
        &new_config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: None,
            max_age: None,
            scope: CacheGcScope::OrphansOnly,
        },
    )
    .await
    .unwrap();
    assert_eq!(retry.namespaces_removed, 1);
    assert!(!npm_priv_file.exists());
    let marker_after_retry =
        std::fs::read_to_string(cache.join(pnpr_config::CACHE_SECRET_MARKER)).unwrap();
    assert_eq!(
        marker_after_retry.trim(),
        pnpr_config::cache_secret_fingerprint(&new_config.cache_secret),
    );
}

#[cfg(unix)]
#[tokio::test]
async fn unreadable_marker_keeps_private_resolver_scopes() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    std::fs::create_dir_all(cache_dir).unwrap();
    let marker = cache_dir.join(pnpr_config::CACHE_SECRET_MARKER);
    let prior_fp = pnpr_config::cache_secret_fingerprint(b"prior-secret-32-bytes-long-00001");
    std::fs::write(&marker, &prior_fp).unwrap();
    let private_file = cache_dir.join("resolver-cache/v12/metadata-private/live-scope/pkg.json");
    std::fs::create_dir_all(private_file.parent().unwrap()).unwrap();
    std::fs::write(&private_file, b"{}").unwrap();

    std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o000)).unwrap();
    let can_still_read = std::fs::read(&marker).is_ok();
    let _ = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: None,
            max_age: None,
            scope: CacheGcScope::OrphansOnly,
        },
    )
    .await
    .unwrap();
    std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        !can_still_read,
        "chmod 0000 on .cache-secret did not prevent reads (privileged user); ReadFailed branch was not exercised",
    );
    assert!(
        private_file.exists(),
        "transient marker read error must not wipe live private resolver scopes",
    );
    assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), prior_fp);
}

fn make_single_file_pkg(digest: &str, size: u64) -> PackageFilesIndex {
    let mut files = HashMap::new();
    files.insert(
        "index.js".to_string(),
        CafsFileInfo { digest: digest.to_string(), mode: 0o644, size, checked_at: None },
    );
    PackageFilesIndex {
        manifest: None,
        requires_build: None,
        requires_prepare: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
        tar_integrity: None,
    }
}

#[tokio::test]
async fn files_index_dry_run_matches_apply_near_max_size() {
    use pnpm_store_dir::StoreIndexFormat;

    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let cache_dir = &config.storage.cache_dir;
    std::fs::create_dir_all(cache_dir).unwrap();
    let marker = cache_dir.join(pnpr_config::CACHE_SECRET_MARKER);
    std::fs::write(&marker, pnpr_config::cache_secret_fingerprint(&config.cache_secret)).unwrap();

    let store = StoreDir::new(cache_dir.join("resolver-store").join(pnpm_store_dir::STORE_VERSION));
    std::fs::create_dir_all(store.root()).unwrap();
    StoreIndex::apply_format(store.root(), StoreIndexFormat::Files).unwrap();

    let old_cas = store.root().join("files/aa/1111111111");
    let fresh_cas = store.root().join("files/bb/2222222222");
    create_file_with_age(&old_cas, &[b'a'; 64], Duration::from_hours(48));
    create_file_with_age(&fresh_cas, &[b'b'; 64], Duration::from_secs(0));

    let old_key = "sha512-old\told-pkg@1.0.0";
    let fresh_key = "sha512-fresh\tfresh-pkg@1.0.0";
    let mut index = StoreIndex::open_in(&store).unwrap();
    index
        .set(old_key, &make_single_file_pkg("aa1111111111", 64))
        .unwrap();
    index
        .set(fresh_key, &make_single_file_pkg("bb2222222222", 64))
        .unwrap();
    index
        .set_aliases(&[("sha512-tar-old\told-pkg@1.0.0".to_string(), old_key.to_string())])
        .unwrap();
    let old_index_bytes = index
        .files_bytes_for_keys(&[old_key.to_string()])
        .unwrap();
    let fresh_index_bytes = index
        .files_bytes_for_keys(&[fresh_key.to_string()])
        .unwrap();
    drop(index);
    assert!(old_index_bytes > 80);

    let marker_len = std::fs::metadata(&marker).unwrap().len();
    // Tight cap: fits marker + fresh_cas + fresh_index_bytes, but would overflow if DryRun forgot
    // to subtract old_index_bytes freed during TTL eviction.
    let tight_limit = marker_len + 64 + fresh_index_bytes + 16;
    assert!(tight_limit < marker_len + 64 + fresh_index_bytes + old_index_bytes);

    let dry = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::DryRun,
            max_size: Some(tight_limit),
            max_age: Some(Duration::from_hours(1)),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();
    let apply = collect_cache(
        &config,
        &CacheGcOptions {
            effect: CacheGcEffect::Apply,
            max_size: Some(tight_limit),
            max_age: Some(Duration::from_hours(1)),
            scope: CacheGcScope::Full,
        },
    )
    .await
    .unwrap();

    assert_eq!(apply.files_evicted, 1);
    assert_eq!(apply.bytes_reclaimed, 64 + old_index_bytes);
    assert_eq!(dry, apply, "dry-run must select the same files and bytes as apply");
    assert!(fresh_cas.exists());
    assert!(
        StoreIndex::open_in(&store)
            .unwrap()
            .get(fresh_key)
            .unwrap()
            .is_some(),
    );
}
