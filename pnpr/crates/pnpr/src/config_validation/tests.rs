use super::validate_startup_config;
use pnpr_config::{BackendConfig, Config, LibsqlSettings, MetadataBackend, SqlBackendSettings};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
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
    config.storage.metadata = MetadataBackend::Sqlite(PathBuf::new());
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
        config.storage.metadata = MetadataBackend::Libsql(libsql_settings(url));
        let err = validate_startup_config(&mut config).unwrap_err().to_string();
        assert!(err.contains("database.url is not a valid URL"), "{url}: {err}");
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
        assert!(err.contains("database.url is not a valid URL"), "{err}");
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
        config.storage.metadata = MetadataBackend::Postgres(sql_settings(url));
        let err = validate_startup_config(&mut config).unwrap_err().to_string();
        assert!(err.contains("database.url is not a valid URL"), "{url}: {err}");
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
        config.storage.metadata = MetadataBackend::Mysql(sql_settings(url));
        let err = validate_startup_config(&mut config).unwrap_err().to_string();
        assert!(err.contains("database.url is not a valid URL"), "{url}: {err}");
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
    config.storage.metadata = MetadataBackend::Postgres(zero_timeout);
    assert!(
        validate_startup_config(&mut config)
            .unwrap_err()
            .to_string()
            .contains("timeout"),
    );
}
