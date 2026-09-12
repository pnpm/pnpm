use super::{
    BackendConfig, Config, Ecosystem, HostedStoreConfig, Path, PathBuf, RegistryError, S3Settings,
    listen, s3_settings_for,
};
use std::fmt::Write as _;

#[test]
fn from_yaml_str_storage_is_resolved_relative_to_base_dir() {
    let yaml = "storage: ./store\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage, PathBuf::from("/etc/pnpr/./store"));
}

#[test]
fn from_yaml_str_absolute_storage_is_left_alone() {
    let yaml = "storage: /var/lib/pnpr\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage, PathBuf::from("/var/lib/pnpr"));
}

#[test]
fn cache_storage_defaults_to_subdir_of_storage() {
    let yaml = "storage: /var/lib/pnpr\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.cache_storage, PathBuf::from("/var/lib/pnpr/.pnpr-cache"));
}

#[test]
fn hosted_store_defaults_to_fs_without_an_s3_block() {
    let yaml = "storage: /var/lib/pnpr\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert!(matches!(config.hosted_store, HostedStoreConfig::Fs));
}

#[test]
fn s3_block_selects_the_object_store_backend_with_normalized_prefix() {
    let yaml = "\
storage: /var/lib/pnpr
s3:
  bucket: my-bucket
  region: auto
  endpoint: https://acct.r2.cloudflarestorage.com
  prefix: packages
  accessKeyId: AKIA-test
  secretAccessKey: secret-test
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.hosted_store {
        HostedStoreConfig::S3(settings) => assert_eq!(settings.normalized_prefix(), "packages/"),
        other => panic!("expected an S3 hosted store, got {other:?}"),
    }
}

#[test]
fn s3_block_without_a_bucket_is_a_config_error() {
    let yaml = "storage: /x\ns3:\n  region: auto\n";
    assert!(Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).is_err());
}

#[test]
fn libsql_backend_block_selects_the_networked_record_store() {
    let yaml = "\
storage: /var/lib/pnpr
backend:
  libsql:
    url: libsql://db.turso.io
    authToken: tok-secret
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.backend {
        BackendConfig::Libsql(settings) => {
            assert_eq!(settings.url, "libsql://db.turso.io");
            assert_eq!(settings.auth_token.as_deref(), Some("tok-secret"));
        }
        other => panic!("expected a libsql backend, got {other:?}"),
    }
}

#[test]
fn libsql_backend_auth_token_is_optional() {
    let yaml = "\
storage: /var/lib/pnpr
backend:
  libsql:
    url: http://127.0.0.1:8080
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.backend {
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
storage: /var/lib/pnpr
backend:
  libsql:
    url: libsql://db.turso.io
    replicaPath: auth-replica.db
    syncIntervalSecs: 15
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.backend {
        BackendConfig::Libsql(settings) => {
            assert_eq!(
                settings.replica_path.as_deref(),
                Some(Path::new("/etc/pnpr/auth-replica.db")),
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
storage: /var/lib/pnpr
backend:
  libsql:
    url: libsql://db.turso.io
    replicaPath: /var/lib/pnpr/auth-replica.db
upstreams: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.backend {
        BackendConfig::Libsql(settings) => assert_eq!(
            settings.replica_path.as_deref(),
            Some(Path::new("/var/lib/pnpr/auth-replica.db")),
        ),
        other => panic!("expected a libsql backend, got {other:?}"),
    }
}

#[test]
fn backend_block_rejects_multiple_database_backends() {
    let yaml = "\
storage: /var/lib/pnpr
backend:
  libsql:
    url: libsql://db.turso.io
  postgres:
    url: postgres://pnpr:secret@db.example/pnpr
upstreams: {}
";
    let err = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None)
        .expect_err("a backend block must not select two databases");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

// ----- serde defaults ---------------------------------------------------

#[test]
fn yaml_with_no_storage_uses_default_storage_string() {
    // `storage:` is absent entirely — `default_storage_string`
    // supplies `"./storage"`, which `resolve_relative` then joins
    // to the config-file's parent dir.
    let yaml = "upstreams: {}\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage, PathBuf::from("/etc/pnpr/./storage"));
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
        secret_access_key: Some("s3cr3t-do-not-log".to_string()),
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
fn ecosystem_groups_scope_names_sources_defaults_and_hosted_storage() {
    let mut yaml = String::from("registries:\n");
    for ecosystem in Ecosystem::all() {
        write!(yaml,
            "  {ecosystem}:\n    internal:\n      type: hosted\n    main:\n      type: router\n      sources: [internal]\n",
        ).unwrap();
    }
    yaml.push_str("defaultRegistry:\n  npm: main\n  cargo: main\n  pypi: main\n  oci: main\n");
    let mut config = Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None).unwrap();
    config.ensure_valid_registry_graph().unwrap();
    for ecosystem in Ecosystem::all() {
        let key = format!("{ecosystem}/internal");
        let router = format!("{ecosystem}/main");
        assert_eq!(config.registries.default_for(ecosystem), Some(router.as_str()));
        assert_eq!(config.registries.sources("main", ecosystem), [key.as_str()]);
        assert_eq!(config.hosted[&key].org, format!("{ecosystem}~internal"));
        for resolution in [
            config.registries.resolve("internal", ecosystem, "demo"),
            config.registries.resolve("main", ecosystem, "demo"),
            config.registries.resolve_default(ecosystem, "demo"),
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
    assert_eq!(config.registries.addressed("cargo/internal", Ecosystem::Npm), None);
}
