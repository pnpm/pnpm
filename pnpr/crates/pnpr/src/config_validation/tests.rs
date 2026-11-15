use super::validate_startup_config;
use object_store::memory::InMemory;
use pnpr_config::{
    BackendConfig, Config, DatabaseBackend, HostedStoreConfig, LibsqlSettings, S3Settings,
    SqlBackendSettings,
};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

fn base_config() -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    Config::from_default_yaml(Path::new("/etc/pnpr"), listen, None)
}

fn sql_settings(url: &str) -> SqlBackendSettings {
    SqlBackendSettings {
        url: url.to_string(),
        max_connections: Some(4),
        timeout: Duration::from_secs(30),
        startup_timeout: Duration::from_mins(1),
    }
}

fn libsql_settings(url: &str) -> LibsqlSettings {
    LibsqlSettings {
        url: url.to_string(),
        auth_token: None,
        replica_path: None,
        sync_interval_secs: None,
        max_connections: Some(4),
    }
}

#[test]
fn rejects_empty_sqlite_path_on_programmatic_config() {
    let mut config = base_config();
    config.storage.database = DatabaseBackend::Sqlite(PathBuf::new());
    let err = validate_startup_config(&mut config).unwrap_err().to_string();
    assert!(err.contains("sqlite:"), "{err}");
}

#[test]
fn rejects_invalid_libsql_urls_without_leaking_credentials() {
    for url in [
        "libsql://",
        "libsql:///missing-host",
        "postgres://user:supersecret@db.example.test/pnpr",
        "libsql:user:supersecret@db.example.test",
    ] {
        let mut config = base_config();
        config.storage.database = DatabaseBackend::Libsql(libsql_settings(url));
        let err = validate_startup_config(&mut config).unwrap_err().to_string();
        assert!(err.contains("storage.database.url is not a valid URL"), "{url}: {err}");
        assert!(!err.contains("supersecret"), "{url}: {err}");
    }
}

#[test]
fn rejects_mismatched_schemes_and_invalid_urls_on_identity_backend() {
    for backend in [
        BackendConfig::Postgres(sql_settings("mysql://user:supersecret@db.example.test/pnpr")),
        BackendConfig::Postgres(sql_settings("postgres:user:supersecret@db.example.test/pnpr")),
        BackendConfig::Mysql(sql_settings("postgres://user:supersecret@db.example.test/pnpr")),
        BackendConfig::Mysql(sql_settings("mysql:user:supersecret@db.example.test/pnpr")),
        BackendConfig::Libsql(libsql_settings("libsql://")),
    ] {
        let mut config = base_config();
        config.identity.backend = backend;
        let err = validate_startup_config(&mut config).unwrap_err().to_string();
        assert!(err.contains("storage.database.url is not a valid URL"), "{err}");
        assert!(!err.contains("supersecret"), "{err}");
    }
}

#[cfg(feature = "backend-postgres")]
#[test]
fn rejects_invalid_postgres_connect_options_without_leaking_credentials() {
    for url in [
        "postgres://user:supersecret@db.example.test/pnpr?sslmode=bogus",
        "postgresql://user:supersecret@db.example.test:99999/pnpr",
    ] {
        let mut config = base_config();
        config.storage.database = DatabaseBackend::Postgres(sql_settings(url));
        let err = validate_startup_config(&mut config).unwrap_err().to_string();
        assert!(err.contains("storage.database.url is not a valid URL"), "{url}: {err}");
        assert!(!err.contains("supersecret"), "{url}: {err}");
    }
}

#[cfg(feature = "backend-mysql")]
#[test]
fn rejects_invalid_mysql_connect_options_without_leaking_credentials() {
    for url in [
        "mysql://user:supersecret@db.example.test/pnpr?ssl-mode=bogus",
        "mysql://user:supersecret@db.example.test:99999/pnpr",
    ] {
        let mut config = base_config();
        config.storage.database = DatabaseBackend::Mysql(sql_settings(url));
        let err = validate_startup_config(&mut config).unwrap_err().to_string();
        assert!(err.contains("storage.database.url is not a valid URL"), "{url}: {err}");
        assert!(!err.contains("supersecret"), "{url}: {err}");
    }
}

#[test]
fn rejects_zero_pool_limits_and_timeouts() {
    let mut zero_pool = sql_settings("postgres://db.example.test/pnpr");
    zero_pool.max_connections = Some(0);
    let mut config = base_config();
    config.identity.backend = BackendConfig::Postgres(zero_pool);
    assert!(
        validate_startup_config(&mut config)
            .unwrap_err()
            .to_string()
            .contains("maxConnections"),
    );

    let mut zero_timeout = sql_settings("postgres://db.example.test/pnpr");
    zero_timeout.timeout = Duration::ZERO;
    let mut config = base_config();
    config.storage.database = DatabaseBackend::Postgres(zero_timeout);
    assert!(
        validate_startup_config(&mut config)
            .unwrap_err()
            .to_string()
            .contains("timeout"),
    );

    let mut zero_shutdown = base_config();
    zero_shutdown.http.shutdown_timeout = Duration::ZERO;
    assert!(
        validate_startup_config(&mut zero_shutdown)
            .unwrap_err()
            .to_string()
            .contains("shutdownTimeout"),
    );
}

#[test]
fn rejects_shared_backends_without_configured_secret() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s3_hosted = base_config();
    s3_hosted.storage.cache_dir = tmp.path().join("cache-s3");
    s3_hosted.storage.hosted_backend = HostedStoreConfig::S3(S3Settings {
        bucket: "packages".to_string(),
        region: Some("auto".to_string()),
        endpoint: Some("https://s3.example.test".to_string()),
        prefix: None,
        access_key_id: None,
        secret_access_key: None,
        force_path_style: None,
        allow_http: None,
    });
    let err = validate_startup_config(&mut s3_hosted).unwrap_err().to_string();
    assert!(err.contains("`secret` or `secret.cache` must be configured"), "{err}");

    let mut obj_hosted = base_config();
    obj_hosted.storage.cache_dir = tmp.path().join("cache-obj");
    obj_hosted.storage.hosted_backend =
        HostedStoreConfig::ObjectStore { store: Arc::new(InMemory::new()), prefix: String::new() };
    let err = validate_startup_config(&mut obj_hosted).unwrap_err().to_string();
    assert!(err.contains("`secret` or `secret.cache` must be configured"), "{err}");

    #[cfg(feature = "backend-libsql")]
    {
        let mut shared_db = base_config();
        shared_db.storage.cache_dir = tmp.path().join("cache-libsql");
        shared_db.storage.database =
            DatabaseBackend::Libsql(libsql_settings("libsql://db.example.test"));
        let err = validate_startup_config(&mut shared_db).unwrap_err().to_string();
        assert!(err.contains("`secret` or `secret.cache` must be configured"), "{err}");

        shared_db.has_configured_secret = true;
        shared_db.cache_secret = Arc::from(&b"configured-cache-secret-16bytes!"[..]);
        validate_startup_config(&mut shared_db).unwrap();
    }
}

#[test]
fn rejects_persisted_or_shared_cache_dir_without_configured_secret() {
    let tmp = tempfile::tempdir().unwrap();

    let empty_cache = tmp.path().join("empty-cache");
    std::fs::create_dir_all(empty_cache.join("lost+found")).unwrap();
    let mut fresh = base_config();
    fresh.storage.cache_dir = empty_cache;
    validate_startup_config(&mut fresh).unwrap();

    let populated_cache = tmp.path().join("populated-cache");
    std::fs::create_dir_all(&populated_cache).unwrap();
    std::fs::write(populated_cache.join("layout.json"), b"{\"layout\":1}").unwrap();
    let mut persisted = base_config();
    persisted.storage.cache_dir = populated_cache;
    let err = validate_startup_config(&mut persisted).unwrap_err().to_string();
    assert!(err.contains("when the cache directory persists across restarts"), "{err}");

    persisted.has_configured_secret = true;
    persisted.cache_secret = Arc::from(&b"configured-cache-secret-16bytes!"[..]);
    validate_startup_config(&mut persisted).unwrap();

    let marked_cache = tmp.path().join("marked-cache");
    std::fs::create_dir_all(&marked_cache).unwrap();
    std::fs::write(
        marked_cache.join(pnpr_config::CACHE_SECRET_MARKER),
        pnpr_config::cache_secret_fingerprint(b"previous-ephemeral-secret-32byte"),
    )
    .unwrap();
    let mut restarted = base_config();
    restarted.storage.cache_dir = marked_cache;
    let err = validate_startup_config(&mut restarted).unwrap_err().to_string();
    assert!(err.contains("when the cache directory persists across restarts"), "{err}");
}

#[test]
fn rejects_cache_overlapping_hosted_storage_or_sqlite_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let storage = tmp.path().join("storage");
    std::fs::create_dir_all(&storage).unwrap();

    let mut valid_subdir = base_config();
    valid_subdir.has_configured_secret = true;
    valid_subdir.cache_secret = Arc::from(&b"configured-cache-secret-16bytes!"[..]);
    valid_subdir.storage.hosted_dir = storage.clone();
    valid_subdir.storage.database =
        DatabaseBackend::Sqlite(pnpr_config::default_database_path(&storage));
    valid_subdir.storage.cache_dir = storage.join("cache");
    validate_startup_config(&mut valid_subdir).unwrap();

    let mut equal_paths = valid_subdir.clone();
    equal_paths.storage.cache_dir = storage;
    let err = validate_startup_config(&mut equal_paths).unwrap_err().to_string();
    assert!(err.contains("cache path must not equal or contain"), "{err}");

    let mut cache_contains_storage = valid_subdir.clone();
    cache_contains_storage.storage.cache_dir = tmp.path().to_path_buf();
    let err = validate_startup_config(&mut cache_contains_storage).unwrap_err().to_string();
    assert!(err.contains("cache path must not equal or contain"), "{err}");

    let mut sqlite_in_cache = valid_subdir;
    sqlite_in_cache.storage.cache_dir = tmp.path().join("separate-cache");
    sqlite_in_cache.storage.database =
        DatabaseBackend::Sqlite(sqlite_in_cache.storage.cache_dir.join("metadata.sqlite"));
    let err = validate_startup_config(&mut sqlite_in_cache).unwrap_err().to_string();
    assert!(err.contains("storage.database.url SQLite path must not live inside"), "{err}");

    sqlite_in_cache.features.registry.enabled = false;
    sqlite_in_cache.features.resolver.enabled = true;
    let err = validate_startup_config(&mut sqlite_in_cache).unwrap_err().to_string();
    assert!(err.contains("storage.database.url SQLite path must not live inside"), "{err}");
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_cache_paths_including_dot_dot_segments() {
    let tmp = tempfile::tempdir().unwrap();
    let storage = tmp.path().join("storage");
    std::fs::create_dir_all(&storage).unwrap();

    let mut base = base_config();
    base.has_configured_secret = true;
    base.cache_secret = Arc::from(&b"configured-cache-secret-16bytes!"[..]);
    base.storage.hosted_dir = storage.clone();
    base.storage.database = DatabaseBackend::Sqlite(pnpr_config::default_database_path(&storage));

    let link = tmp.path().join("cache-link");
    std::os::unix::fs::symlink(&storage, &link).unwrap();
    let mut symlink_equal = base.clone();
    symlink_equal.storage.cache_dir = link;
    let err = validate_startup_config(&mut symlink_equal).unwrap_err().to_string();
    assert!(err.contains("cache path must not equal or contain"), "{err}");

    let else_deep = tmp.path().join("else/deep");
    let else_hosted = tmp.path().join("else/hosted");
    std::fs::create_dir_all(&else_deep).unwrap();
    std::fs::create_dir_all(&else_hosted).unwrap();
    let deep_link = tmp.path().join("deep-link");
    std::os::unix::fs::symlink(&else_deep, &deep_link).unwrap();

    let mut dotdot = base;
    dotdot.storage.cache_dir = tmp.path().join("else");
    dotdot.storage.hosted_dir = deep_link.join("../hosted");
    dotdot.storage.database = DatabaseBackend::Sqlite(tmp.path().join("outside-meta.sqlite"));
    let err = validate_startup_config(&mut dotdot).unwrap_err().to_string();
    assert!(err.contains("cache path must not equal or contain"), "{err}");
}

#[test]
fn rejects_libsql_replica_and_osv_paths_inside_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let storage = tmp.path().join("storage");
    let cache = tmp.path().join("cache");

    let mut base = base_config();
    base.has_configured_secret = true;
    base.cache_secret = Arc::from(&b"configured-cache-secret-16bytes!"[..]);
    base.storage.hosted_dir = storage.clone();
    base.storage.cache_dir = cache.clone();
    base.storage.database = DatabaseBackend::Sqlite(pnpr_config::default_database_path(&storage));

    let mut meta_replica = base.clone();
    let mut libsql = libsql_settings("libsql://db.example.test");
    libsql.replica_path = Some(cache.join("meta-replica.db"));
    libsql.sync_interval_secs = Some(5);
    meta_replica.storage.database = DatabaseBackend::Libsql(libsql);
    let err = super::validate_cache_paths(&meta_replica).unwrap_err().to_string();
    assert!(err.contains("libSQL replicaPath must not live inside"), "{err}");

    let mut auth_replica = base.clone();
    let mut auth_libsql = libsql_settings("libsql://auth.example.test");
    auth_libsql.replica_path = Some(cache.join("auth-replica.db"));
    auth_libsql.sync_interval_secs = Some(5);
    auth_replica.identity.backend = BackendConfig::Libsql(auth_libsql);
    let err = super::validate_cache_paths(&auth_replica).unwrap_err().to_string();
    assert!(err.contains("libSQL replicaPath must not live inside"), "{err}");

    let mut osv_in_cache = base.clone();
    osv_in_cache.osv.path = Some(cache.join("osv/all.zip"));
    let err = validate_startup_config(&mut osv_in_cache).unwrap_err().to_string();
    assert!(err.contains("osv.path must not live inside or contain"), "{err}");

    let mut default_osv_overlap = base;
    default_osv_overlap.osv.path = None;
    default_osv_overlap.storage.cache_dir = storage.join(".osv");
    let err = validate_startup_config(&mut default_osv_overlap).unwrap_err().to_string();
    assert!(err.contains("osv.path must not live inside or contain"), "{err}");
}
