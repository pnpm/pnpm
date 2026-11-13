use super::{
    BackendConfig, Config, Ecosystem, FeatureOverrides, HostedStoreConfig, Path, PathBuf,
    RegistryError, S3Settings, listen, load_with_graph, s3_settings_for,
};
use std::fmt::Write as _;

#[test]
fn from_yaml_str_storage_is_resolved_relative_to_base_dir() {
    let yaml = "storage: ./store\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage.hosted_dir, PathBuf::from("/etc/pnpr/./store"));
}

#[test]
fn from_yaml_str_absolute_storage_is_left_alone() {
    let yaml = "storage: /var/lib/pnpr\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage.hosted_dir, PathBuf::from("/var/lib/pnpr"));
}

#[test]
fn storage_table_names_the_directory_with_path() {
    let yaml = "storage:\n  path: ./store\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage.hosted_dir, PathBuf::from("/etc/pnpr/./store"));
    assert_eq!(config.storage.cache_dir, PathBuf::from("/etc/pnpr/./store/.pnpr-cache"));
}

#[test]
fn storage_table_without_a_path_uses_the_default_directory() {
    let yaml = "storage:\n  database:\n    url: sqlite:./meta.db\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage.hosted_dir, PathBuf::from("/etc/pnpr/./storage"));
    assert_eq!(
        config.storage.database,
        crate::DatabaseBackend::Sqlite(PathBuf::from("/etc/pnpr/./meta.db")),
    );
}

#[test]
fn storage_rejects_unknown_fields_and_the_top_level_backend_keys() {
    let cases = [
        ("storage:\n  bucket: b\n", "unknown field `bucket`"),
        ("s3:\n  bucket: b\n", "unknown field `s3`"),
        ("database:\n  url: sqlite:./m.db\n", "unknown field `database`"),
        ("storage: [a]\n", "a storage path or a table"),
    ];
    for (yaml, expected) in cases {
        let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap_err();
        assert!(err.to_string().contains(expected), "{yaml:?}: {err}");
    }
}

#[test]
fn cache_storage_defaults_to_subdir_of_storage() {
    let yaml = "storage: /var/lib/pnpr\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage.cache_dir, PathBuf::from("/var/lib/pnpr/.pnpr-cache"));
}

#[test]
fn hosted_store_defaults_to_fs_without_an_s3_block() {
    let yaml = "storage: /var/lib/pnpr\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert!(matches!(config.storage.hosted_backend, HostedStoreConfig::Fs));
}

#[test]
fn s3_block_selects_the_object_store_backend_with_normalized_prefix() {
    let yaml = "\
storage:
  path: /var/lib/pnpr
  s3:
    bucket: my-bucket
    region: auto
    endpoint: https://acct.r2.cloudflarestorage.com
    prefix: packages
    accessKeyId: AKIA-test
    secretAccessKey: secret-test
  database:
    url: libsql://db.example.turso.io
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.storage.hosted_backend {
        HostedStoreConfig::S3(settings) => assert_eq!(settings.normalized_prefix(), "packages/"),
        other => panic!("expected an S3 hosted store, got {other:?}"),
    }
}

#[test]
fn s3_block_without_a_bucket_is_a_config_error() {
    let yaml = "storage:\n  path: /x\n  s3:\n    region: auto\n";
    assert!(Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).is_err());
}

#[test]
fn libsql_backend_block_selects_the_networked_record_store() {
    let yaml = "\
storage:
  path: /var/lib/pnpr
  database:
    url: libsql://db.turso.io
    authToken: tok-secret
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.identity.backend {
        BackendConfig::Libsql(settings) => {
            assert_eq!(settings.url, "libsql://db.turso.io");
            assert_eq!(settings.auth_token.as_deref(), Some("tok-secret"));
        }
        other => panic!("expected a libsql backend, got {other:?}"),
    }
}

#[test]
fn libsql_backend_reads_max_connections() {
    let yaml = "\
storage:
  path: /var/lib/pnpr
  database:
    url: http://127.0.0.1:8080
    maxConnections: 3
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.identity.backend {
        BackendConfig::Libsql(settings) => assert_eq!(settings.max_connections, Some(3)),
        other => panic!("expected a libsql backend, got {other:?}"),
    }
}

#[test]
fn libsql_backend_rejects_an_empty_pool() {
    let yaml = "\
storage:
  path: /var/lib/pnpr
  database:
    url: http://127.0.0.1:8080
    maxConnections: 0
";
    let error = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap_err();
    assert!(error.to_string().contains("maxConnections must be greater than 0"), "{error}");
}

#[test]
fn libsql_backend_auth_token_is_optional() {
    let yaml = "\
storage:
  path: /var/lib/pnpr
  database:
    url: http://127.0.0.1:8080
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.identity.backend {
        BackendConfig::Libsql(settings) => {
            assert!(settings.auth_token.is_none());
            assert!(settings.replica_path.is_none(), "no replica by default");
        }
        other => panic!("expected a libsql backend, got {other:?}"),
    }
}

#[test]
fn libsql_backend_resolves_relative_replica_path_against_config_dir() {
    let yaml = "\
storage:
  path: /var/lib/pnpr
  database:
    url: libsql://db.turso.io
    replicaPath: auth-replica.db
    syncInterval: 15s
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.identity.backend {
        BackendConfig::Libsql(settings) => {
            assert_eq!(
                settings.replica_path.as_deref(),
                Some(Path::new("/etc/pnpr/auth-replica.db.auth")),
                "a relative replicaPath resolves against the config file's directory",
            );
            assert_eq!(settings.sync_interval_secs, Some(15));
        }
        other => panic!("expected a libsql backend, got {other:?}"),
    }
}

#[test]
fn libsql_backend_keeps_absolute_replica_path() {
    let yaml = "\
storage:
  path: /var/lib/pnpr
  database:
    url: libsql://db.turso.io
    replicaPath: /var/lib/pnpr/auth-replica.db
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.identity.backend {
        BackendConfig::Libsql(settings) => assert_eq!(
            settings.replica_path.as_deref(),
            Some(Path::new("/var/lib/pnpr/auth-replica.db.auth")),
        ),
        other => panic!("expected a libsql backend, got {other:?}"),
    }
}

#[test]
fn backend_block_rejects_multiple_database_backends() {
    let yaml = "\
storage:
  path: /var/lib/pnpr
  database:
    url: libsql://db.turso.io
    postgres:
      url: postgres://pnpr:secret@db.example/pnpr
";
    let err = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None)
        .expect_err("a backend block must not select two databases");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn libsql_backend_rejects_subsecond_sync_interval() {
    let yaml = "\
storage:
  path: /var/lib/pnpr
  database:
    url: libsql://db.turso.io
    syncInterval: 500ms
";
    let err = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None)
        .expect_err("subsecond syncInterval must be rejected");
    assert!(err.to_string().contains("storage.database.syncInterval"), "{err}");
}

// ----- serde defaults ---------------------------------------------------

#[test]
fn yaml_with_no_storage_uses_default_storage_string() {
    // `storage:` is absent entirely — `default_storage_string`
    // supplies `"./storage"`, which `resolve_relative` then joins
    // to the config-file's parent dir.
    let yaml = "cors: {}\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage.hosted_dir, PathBuf::from("/etc/pnpr/./storage"));
}

/// `Debug` on [`S3Settings`] is reachable from `Debug` on the whole [`Config`],
/// so a diagnostic dump must not carry the operator's S3 credentials.
#[test]
fn s3_settings_debug_redacts_credentials() {
    let settings = S3Settings {
        bucket: "packages".to_string(),
        region: Some("auto".to_string()),
        endpoint: None,
        prefix: None,
        access_key_id: Some("AKIAEXAMPLEKEYID".to_string()),
        secret_access_key: Some("s3cr3t-do-not-log".into()),
        force_path_style: None,
        allow_http: None,
    };

    let rendered = format!("{settings:?}");
    assert!(!rendered.contains("s3cr3t-do-not-log"), "secret leaked: {rendered}");
    assert!(!rendered.contains("AKIAEXAMPLEKEYID"), "key id leaked: {rendered}");
    assert!(rendered.contains("<redacted>"), "expected redaction marker: {rendered}");
    // Non-secret fields still render, so the dump stays useful.
    assert!(rendered.contains("packages"), "bucket should render: {rendered}");
}

/// `endpoint` is operator-supplied, so it can carry `user:pass@` userinfo or a
/// token query parameter. Masking only the key fields would leave that path
/// open.
#[test]
fn s3_settings_debug_redacts_credentials_inside_the_endpoint() {
    let mut settings = s3_settings_for(Some("https://minio:hunter2@minio.corp.example"), None);
    settings.bucket = "packages".to_string();

    let rendered = format!("{settings:?}");
    assert!(!rendered.contains("hunter2"), "endpoint userinfo leaked: {rendered}");
    assert!(rendered.contains("minio.corp.example"), "host should still render: {rendered}");
}

#[test]
fn explicit_ecosystems_scope_defaults_and_hosted_storage() {
    let mut yaml = String::from("registries:\n");
    for ecosystem in Ecosystem::all() {
        write!(yaml,
            "  {ecosystem}-internal:\n    type: hosted\n    ecosystem: {ecosystem}\n    namespace: internal\n  {ecosystem}-main:\n    type: router\n    sources: [{ecosystem}-internal]\n",
        ).unwrap();
    }
    yaml.push_str("defaultRegistry:\n  npm: npm-main\n  cargo: cargo-main\n  pypi: pypi-main\n  oci: oci-main\n");
    let mut config = load_with_graph(&yaml, Path::new("/x"), listen(), None).unwrap();
    config.routing.ensure_valid_registry_graph(config.features.registry.enabled).unwrap();
    for ecosystem in Ecosystem::all() {
        let key = format!("{ecosystem}-internal");
        let router = format!("{ecosystem}-main");
        assert_eq!(config.routing.registries.default_for(ecosystem), Some(router.as_str()));
        assert_eq!(config.routing.registries.sources(&router, ecosystem), [key.as_str()]);
        assert_eq!(config.routing.hosted[&key].namespace, "internal");
        for resolution in [
            config.routing.registries.resolve(&key, ecosystem, "demo"),
            config.routing.registries.resolve(&router, ecosystem, "demo"),
            config.routing.registries.resolve_default(ecosystem, "demo"),
        ] {
            assert_eq!(
                resolution,
                pnpr_registry::Resolved::Concrete {
                    registry: key.as_str(),
                    kind: pnpr_registry::ConcreteKind::Hosted,
                },
            );
        }
    }
    assert_eq!(config.routing.registries.addressed("cargo-internal", Ecosystem::Npm), None);
}

#[test]
fn cache_settings_parsed_from_table() {
    let yaml = "\
storage: /var/lib/pnpr
cache:
  path: /custom/cache
  maxSize: 10GB
  maxAge: 30d
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage.cache_dir, PathBuf::from("/custom/cache"));
    assert_eq!(config.storage.cache.max_size, Some(10 * 1024 * 1024 * 1024));
    assert_eq!(config.storage.cache.max_age, Some(std::time::Duration::from_hours(720)));
}

#[test]
fn cache_settings_defaults_path_when_omitted_in_table() {
    let yaml = "\
storage: /var/lib/pnpr
cache:
  maxSize: 500MB
  maxAge: 12h
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage.cache_dir, PathBuf::from("/var/lib/pnpr/.pnpr-cache"));
    assert_eq!(config.storage.cache.max_size, Some(500 * 1024 * 1024));
    assert_eq!(config.storage.cache.max_age, Some(std::time::Duration::from_hours(12)));
}

#[test]
fn cache_rejects_invalid_max_size_or_age() {
    let bad_size = "storage: /x\ncache:\n  maxSize: not-a-size\n";
    assert!(Config::from_yaml_str(bad_size, Path::new("/x"), listen(), None).is_err());

    let bad_age = "storage: /x\ncache:\n  maxAge: whenever\n";
    assert!(Config::from_yaml_str(bad_age, Path::new("/x"), listen(), None).is_err());
}

#[test]
fn cache_rejects_unknown_fields() {
    let yaml = "storage: /x\ncache:\n  unknownField: 123\n";
    assert!(Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).is_err());
}

#[test]
fn metadata_defaults_to_sqlite_in_storage() {
    let config =
        Config::from_yaml_str("storage: /var/lib/pnpr\n", Path::new("/"), listen(), None).unwrap();
    let expected = PathBuf::from("/var/lib/pnpr/.metadata/metadata.sqlite");
    assert_eq!(config.storage.database, crate::DatabaseBackend::Sqlite(expected));
}

#[test]
fn an_object_store_without_a_metadata_url_is_refused() {
    let yaml =
        "storage:\n  path: /s\n  s3:\n    bucket: b\nregistries:\n  local:\n    type: hosted\n";
    let err = load_with_graph(yaml, Path::new("/"), listen(), None).unwrap_err();
    let RegistryError::InvalidConfig { reason } = err else { panic!("{err:?}") };
    assert!(reason.contains("storage.database.url"), "{reason}");
}

/// An artifact-only tier (`--disable-registry`) keeps artifacts in the
/// bucket and reads no package metadata, so it needs no metadata URL.
#[test]
fn an_object_store_that_hosts_no_packages_needs_no_metadata_url() {
    let yaml = "storage:\n  path: /s\n  s3:\n    bucket: b\nresolver:\n  enabled: false\nartifacts:\n  enabled: true\n";
    let config = Config::from_yaml_str_with_overrides(
        yaml,
        Path::new("/"),
        Some(listen()),
        None,
        FeatureOverrides { disable_registry: true, ..FeatureOverrides::default() },
    )
    .unwrap();
    assert_eq!(
        config.storage.database,
        crate::DatabaseBackend::Sqlite(PathBuf::from("/s/.metadata/metadata.sqlite")),
    );
}

#[test]
fn a_database_backend_selector_is_refused() {
    for yaml in [
        "storage:\n  path: /s\n  database:\n    backend: documents\n",
        "storage:\n  path: /s\n  database:\n    backend: documents\n    url: sqlite:./m.db\n",
        "storage:\n  path: /s\n  database:\n    backend: sqlite\n",
    ] {
        let err = Config::from_yaml_str(yaml, Path::new("/"), listen(), None).unwrap_err();
        let RegistryError::InvalidConfig { reason } = err else { panic!("{yaml}: {err:?}") };
        assert!(reason.contains("unknown field `backend`"), "{yaml}: {reason}");
    }
}

#[test]
fn a_sqlite_url_resolves_against_the_config_directory() {
    let yaml = "storage:\n  path: /var/lib/pnpr\n  database:\n    url: sqlite:./meta.db\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    let expected = PathBuf::from("/etc/pnpr/./meta.db");
    assert_eq!(config.storage.database, crate::DatabaseBackend::Sqlite(expected));
}

#[test]
fn a_metadata_url_is_refused_when_it_cannot_serve() {
    let with_s3 =
        "storage:\n  path: /s\n  s3:\n    bucket: b\n  database:\n    url: sqlite:./m.db\n";
    let no_path = "storage:\n  path: /s\n  database:\n    url: 'sqlite:'\n";
    let unknown = "storage:\n  path: /s\n  database:\n    url: unsupported://db/pnpr\n";
    let old_path = "storage:\n  path: /s\n  database:\n    path: ./m.db\n";
    for yaml in [with_s3, no_path, unknown, old_path] {
        let err = Config::from_yaml_str(yaml, Path::new("/"), listen(), None).unwrap_err();
        assert!(matches!(err, RegistryError::InvalidConfig { .. }), "{yaml}: {err:?}");
    }
}

#[test]
fn a_libsql_url_carries_its_replica_settings_with_any_hosted_store() {
    let yaml = "\
storage:
  path: /s
  s3:
    bucket: b
  database:
    url: libsql://db.example.turso.io
    authToken: token
    replicaPath: ./replica.db
    syncInterval: 5s
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    let crate::DatabaseBackend::Libsql(settings) = config.storage.database else {
        panic!("libSQL metadata, got {:?}", config.storage.database);
    };
    assert_eq!(settings.url, "libsql://db.example.turso.io");
    assert_eq!(settings.auth_token.as_deref(), Some("token"));
    assert_eq!(settings.replica_path, Some(PathBuf::from("/etc/pnpr/replica.db")));
    assert_eq!(settings.sync_interval_secs, Some(5));
}

#[test]
fn libsql_settings_need_a_libsql_url_and_a_replica_needs_a_sync_interval() {
    let stray =
        "storage:\n  path: /s\n  database:\n    url: sqlite:./m.db\n    replicaPath: ./r.db\n";
    let unbounded = "\
storage:
  path: /s
  database:
    url: http://127.0.0.1:8080
    replicaPath: ./r.db
    syncInterval: 0s
";
    let no_url =
        "storage:\n  path: /s\n  database:\n    authToken: token\n    replicaPath: ./r.db\n";
    for yaml in [stray, unbounded, no_url] {
        let err = Config::from_yaml_str(yaml, Path::new("/"), listen(), None).unwrap_err();
        assert!(matches!(err, RegistryError::InvalidConfig { .. }), "{yaml}: {err:?}");
    }
}

#[test]
fn network_sql_urls_require_double_slash_authority() {
    for raw in [
        "postgres:user:secret@db.example.test/pnpr",
        "postgresql:user:secret@db.example.test/pnpr",
        "mysql:user:secret@db.example.test/pnpr",
    ] {
        let yaml = format!("storage:\n  path: /s\n  database:\n    url: {raw}\n");
        let err = Config::from_yaml_str(&yaml, Path::new("/"), listen(), None).unwrap_err();
        let rendered = err.to_string();
        assert!(matches!(err, RegistryError::InvalidConfig { .. }), "{raw}: {rendered}");
        assert!(!rendered.contains("secret"), "{raw}: {rendered}");
    }
}
