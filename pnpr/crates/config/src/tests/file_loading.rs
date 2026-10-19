use super::{
    Config, DEFAULT_CONFIG_YAML, Ecosystem, LogFormat, LogLevel, Path, RegistryError,
    config_file_in, listen, parse_interval, write_yaml,
};

#[test]
fn from_yaml_str_parses_the_resolver_toggle() {
    let yaml = "
registries:
  npmjs: { type: upstream, url: https://registry.npmjs.org/, public: true }
resolver:
  enabled: false
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(config.features.registry.enabled);
    assert!(!config.features.resolver.enabled);
}

#[test]
fn from_yaml_str_parses_the_allowed_private_networks() {
    let yaml = "
registries:
  npmjs: { type: upstream, url: https://registry.npmjs.org/, public: true }
routes:
  allowedPrivateNetworks: [10.20.0.0/16, fd00::1]
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let networks = &config.routing.route_policy.allowed_private_networks;
    assert_eq!(networks.len(), 2);
    assert!(networks[0].contains("10.20.3.4".parse().unwrap()));
    assert!(networks[1].contains("fd00::1".parse().unwrap()));

    let invalid = yaml.replace("10.20.0.0/16", "10.20.0.0/40");
    let error = Config::from_yaml_str(&invalid, Path::new("/x"), listen(), None).unwrap_err();
    assert!(error.to_string().contains(r#""10.20.0.0/40" is not an IP address or a CIDR network"#));
}

#[test]
fn from_yaml_str_accepts_string_and_bare_number_intervals() {
    use std::time::Duration;
    // `maxage` is a string, `timeout` a bare number (verdaccio accepts
    // both); the bare number must read as seconds rather than failing to
    // deserialize against the `Option<String>`-shaped field.
    let yaml = r"
registries:
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
    maxage: 10m
    timeout: 45
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let upstream = &config.routing.upstreams["npmjs"];
    assert_eq!(upstream.maxage, Some(Duration::from_mins(10)));
    assert_eq!(upstream.requests.timeout, Duration::from_secs(45));
}

#[test]
fn from_yaml_str_tolerates_unresolved_env_var_references() {
    let yaml = r"
storage: ${PNPR_UNSET_VAR_FOR_TEST}./store
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect("an unresolved ${VAR} is replaced with empty, not an error");
    assert!(config.storage.hosted_dir.ends_with("store"));
}

#[test]
fn from_default_yaml_parses_bundled_file() {
    use pnpr_registry::{ConcreteKind, Resolved};
    let config = Config::from_default_yaml(Path::new("/tmp"), listen(), None);
    assert!(config.routing.upstreams.contains_key("npmjs"));
    assert_eq!(config.routing.upstreams["npmjs"].url, "https://registry.npmjs.org/");
    assert_eq!(config.identity.auth.htpasswd.max_users, super::super::MaxUsers::Disabled);
    assert_eq!(config.routing.registries.default_registry(), Some("npmjs"));
    assert!(config.routing.hosted.contains_key("local"));
    assert_eq!(config.logs.level, LogLevel::Info);
    assert!(!config.has_configured_secret);
    assert_eq!(config.resolution_cache_secret.len(), 32);
    // The bundled file defaults to npmjs, proxying public npm packages.
    for upstream in ["react", "lodash", "test-exclude", "@pnpm/error"] {
        assert_eq!(
            config.routing.registries.resolve_default(Ecosystem::Npm, upstream),
            Resolved::Concrete { registry: "npmjs", kind: ConcreteKind::Upstream },
            "{upstream} must proxy npm",
        );
    }
    assert_eq!(
        config.routing.registries.resolve("local", Ecosystem::Npm, "my-pkg"),
        Resolved::Concrete { registry: "local", kind: ConcreteKind::Hosted },
    );
    let rules = &config.routing.hosted["local"].rules;
    assert!(!rules.access_admits("my-pkg", &super::Identity::Anonymous));
    assert!(rules.access_admits("my-pkg", &super::Identity::user("alice")));
}

#[test]
fn default_yaml_const_matches_what_from_default_parses() {
    // Sanity check: the const is non-empty and round-trips through
    // the parser without panicking — i.e. `from_default_yaml`'s
    // `expect(...)` is not a tripwire under future edits.
    assert!(!DEFAULT_CONFIG_YAML.is_empty());
    let _ = Config::from_default_yaml(Path::new("."), listen(), None);
}

#[test]
fn from_yaml_str_rejects_unknown_sections() {
    let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
web:
  enable: false
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap_err();
    assert!(err.to_string().contains("unknown field `web`"));
}

#[test]
fn from_yaml_str_rejects_removed_blocks_with_migration_guidance() {
    let bad_upstreams = "storage: ./s\nupstreams: {}\n";
    let err = Config::from_yaml_str(bad_upstreams, Path::new("/x"), listen(), None).unwrap_err();
    assert!(err.to_string().contains("the top-level `upstreams:` block was removed"));

    let bad_uplinks = "storage: ./s\nuplinks: {}\n";
    let err = Config::from_yaml_str(bad_uplinks, Path::new("/x"), listen(), None).unwrap_err();
    assert!(err.to_string().contains("the top-level `uplinks:` block is not supported"));

    let bad_packages = "storage: ./s\npackages: {}\n";
    let err = Config::from_yaml_str(bad_packages, Path::new("/x"), listen(), None).unwrap_err();
    assert!(err.to_string().contains("the top-level `packages:` block was removed"));

    let bad_groups = "storage: ./s\ngroups: {}\n";
    let err = Config::from_yaml_str(bad_groups, Path::new("/x"), listen(), None).unwrap_err();
    assert!(err.to_string().contains("the top-level `groups:` block was removed"));
}

#[test]
fn from_yaml_str_public_url_defaults_to_listen_when_none_passed() {
    let yaml = "storage: ./s\n";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.http.public_url, format!("http://{}", listen()));
}

#[test]
fn from_yaml_str_public_url_override_wins() {
    let yaml = "storage: ./s\n";
    let config = Config::from_yaml_str(
        yaml,
        Path::new("/x"),
        listen(),
        Some("http://override.test".to_string()),
    )
    .unwrap();
    assert_eq!(config.http.public_url, "http://override.test");
}

#[test]
fn from_yaml_path_round_trips_through_tempfile() {
    // Exercise the file-reading path (not just the in-memory
    // `from_yaml_str` shortcut). Confirms relative `storage:` is
    // resolved against the *config file's* parent dir.
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("registry.yml");
    std::fs::write(&config_path, "storage: ./store\n").unwrap();
    let config = Config::from_yaml(&config_path, listen(), None).unwrap();
    assert_eq!(config.storage.hosted_dir, dir.path().join("./store"));
}

#[test]
fn from_yaml_path_surfaces_parse_errors_as_invalid_data() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("broken.yml");
    std::fs::write(&config_path, "storage: [not, a, string\n").unwrap();
    let err = Config::from_yaml(&config_path, listen(), None).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn from_yaml_path_propagates_missing_file_errors() {
    let err = Config::from_yaml(Path::new("/no/such/file.yml"), listen(), None).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn logs_default_when_yaml_omits_block() {
    let yaml = "storage: ./s\n";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.logs.format, LogFormat::Pretty);
    assert_eq!(config.logs.level, LogLevel::Info);
    assert_eq!(config.logs.sink, "stdout");
    assert!(config.logs.sink_is_supported());
}

#[test]
fn log_json_format_parses() {
    let yaml = "\
storage: ./s
log:
  type: stdout
  format: json
  level: debug
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.logs.format, LogFormat::Json);
    assert_eq!(config.logs.level, LogLevel::Debug);
}

// ----- config_file_in (existence gating) --------------------------------

#[test]
fn config_file_in_returns_none_for_none_dir() {
    assert!(config_file_in(None).is_none());
}

#[test]
fn config_file_in_returns_none_when_file_is_missing() {
    // A fresh tempdir has no `config.yaml`.
    let dir = tempfile::tempdir().unwrap();
    assert!(config_file_in(Some(dir.path().to_path_buf())).is_none());
}

#[test]
fn config_file_in_returns_path_when_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let expected = dir.path().join("config.yaml");
    std::fs::write(&expected, "storage: ./s\n").unwrap();
    let resolved = config_file_in(Some(dir.path().to_path_buf())).expect("file is present");
    assert_eq!(resolved, expected);
}

#[test]
fn config_file_in_rejects_a_directory_at_the_target() {
    // If `config.yaml` exists but is a directory (or symlink to
    // one, etc.), `is_file()` returns false. Auto-discovery should
    // bail rather than try to read it.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("config.yaml")).unwrap();
    assert!(config_file_in(Some(dir.path().to_path_buf())).is_none());
}

#[test]
fn config_file_in_resolved_file_round_trips_through_from_yaml() {
    // The whole point of returning a path is that `from_yaml` can
    // load it. This is the end-to-end happy path for the
    // auto-discovery flow.
    //
    // The `storage:` value is computed at runtime so it's a
    // genuinely absolute path on whichever OS the test runs on
    // (Windows requires a drive-letter prefix to satisfy
    // `Path::is_absolute()`; a Unix-style "/tmp/auto" is not
    // absolute there and gets joined to the config's parent dir).
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join("registry-storage");
    let yaml = format!(
        "\
storage: {storage}
log:
  type: stdout
  format: json
  level: info
",
        storage = storage.display(),
    );
    std::fs::write(dir.path().join("config.yaml"), yaml).unwrap();
    let path = config_file_in(Some(dir.path().to_path_buf())).unwrap();
    let config = Config::from_yaml(&path, listen(), None).unwrap();
    assert_eq!(config.storage.hosted_dir, storage);
    assert_eq!(config.logs.format, LogFormat::Json);
    assert_eq!(config.logs.level, LogLevel::Info);
}

#[test]
fn resolve_propagates_missing_file_error_for_cli_path() {
    let err =
        Config::resolve(Some(Path::new("/no/such/file.yml")), None, listen(), None).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn resolve_propagates_parse_error_for_cli_path() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_yaml(tmp.path(), "broken.yml", "storage: [not, a, string\n");
    let err = Config::resolve(Some(&path), None, listen(), None).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn resolve_propagates_missing_file_error_for_default_path() {
    // Symmetric to the CLI case — a bad default path is just as
    // fatal as a bad CLI path. (In practice callers only pass a
    // default path that already passed `config_file_in`'s
    // `is_file()` check, so this is a defense-in-depth assertion.)
    let err =
        Config::resolve(None, Some(Path::new("/no/such/file.yml")), listen(), None).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn yaml_log_block_with_no_type_field_uses_default_log_type() {
    // `type:` omitted but `format:` and `level:` present. The
    // `default_log_type` serde default kicks in for the missing
    // field; we don't otherwise care about its value at runtime,
    // we just need the parse to succeed (and the runtime config
    // to reflect the supplied format/level).
    let yaml = "\
storage: ./s
log:
  format: json
  level: warn
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.logs.format, LogFormat::Json);
    assert_eq!(config.logs.level, LogLevel::Warn);
    // `type:` omitted entirely falls back to the supported stdout sink.
    assert_eq!(config.logs.sink, "stdout");
    assert!(config.logs.sink_is_supported());
}

#[test]
fn parse_interval_handles_suffixes_compounds_and_bare_numbers() {
    use std::time::Duration;
    assert_eq!(parse_interval("30s"), Some(Duration::from_secs(30)));
    assert_eq!(parse_interval("2m"), Some(Duration::from_mins(2)));
    assert_eq!(parse_interval("5m"), Some(Duration::from_mins(5)));
    assert_eq!(parse_interval("1h"), Some(Duration::from_hours(1)));
    assert_eq!(parse_interval("1d"), Some(Duration::from_hours(24)));
    assert_eq!(parse_interval("1w"), Some(Duration::from_hours(168)));
    assert_eq!(parse_interval("500ms"), Some(Duration::from_millis(500)));
    // Compound, with and without whitespace.
    assert_eq!(parse_interval("1h30m"), Some(Duration::from_mins(90)));
    assert_eq!(parse_interval("2m 30s"), Some(Duration::from_secs(150)));
    // A bare number is seconds, matching verdaccio's `interval * 1000` ms.
    assert_eq!(parse_interval("45"), Some(Duration::from_secs(45)));
    // A trailing suffix-less number is also seconds.
    assert_eq!(parse_interval("1m15"), Some(Duration::from_secs(75)));
}

#[test]
fn parse_interval_rejects_garbage() {
    assert_eq!(parse_interval(""), None);
    assert_eq!(parse_interval("   "), None);
    assert_eq!(parse_interval("soon"), None);
    assert_eq!(parse_interval("2x"), None);
    assert_eq!(parse_interval("m30"), None);
}

#[test]
fn route_policy_parses_public_routes() {
    let yaml = r"
routes:
  public:
    - registry: https://registry.npmjs.org/
      package: '@babel/*'
    - package: '@types/*'
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.routing.route_policy.public.len(), 2);
    assert_eq!(
        config.routing.route_policy.public[0].registry.as_deref(),
        Some("https://registry.npmjs.org/"),
    );
    assert_eq!(config.routing.route_policy.public[0].package.as_deref(), Some("@babel/*"));
    assert_eq!(config.routing.route_policy.public[1].registry, None);
}

#[test]
fn resolution_secret_uses_yaml_secret_then_falls_back_to_random() {
    let with_secret = Config::from_yaml_str(
        "secret: pnpm-registry-mock-secret-key-32",
        Path::new("/x"),
        listen(),
        None,
    )
    .unwrap();
    assert_eq!(with_secret.resolution_cache_secret.as_ref(), b"pnpm-registry-mock-secret-key-32");

    // No `secret:` yields a fresh 32-byte CSPRNG value.
    let without_secret = Config::from_yaml_str("{}", Path::new("/x"), listen(), None).unwrap();
    assert_eq!(without_secret.resolution_cache_secret.len(), 32);

    // A too-short `secret:` is a config error rather than a weak HMAC key.
    let short = Config::from_yaml_str("secret: short", Path::new("/x"), listen(), None);
    assert!(matches!(short, Err(RegistryError::InvalidConfig { .. })));
}

#[test]
fn config_rejects_unknown_fields_across_subsystems() {
    let cases = [
        ("top-level", "unknownField: value\n", "unknown field `unknownField`"),
        ("auth", "auth:\n  unknownAuth: true\n", "unknown field `unknownAuth`"),
        ("auth.htpasswd", "auth:\n  htpasswd:\n    unknownHt: true\n", "unknown field `unknownHt`"),
        ("auth.tokens", "auth:\n  tokens:\n    unknownTok: true\n", "unknown field `unknownTok`"),
        ("backend", "backend:\n  unknownBackend: {}\n", "unknown field `unknownBackend`"),
        (
            "backend.libsql",
            "backend:\n  libsql:\n    url: libsql://x\n    unknownLibsql: 1\n",
            "unknown field `unknownLibsql`",
        ),
        (
            "backend.postgres",
            "backend:\n  postgres:\n    url: postgres://x\n    unknownPg: 1\n",
            "unknown field `unknownPg`",
        ),
        ("s3", "s3:\n  bucket: b\n  unknownS3: 1\n", "unknown field `unknownS3`"),
        ("osv", "osv:\n  unknownOsv: 1\n", "unknown field `unknownOsv`"),
        ("log", "log:\n  unknownLog: 1\n", "unknown field `unknownLog`"),
        ("routes", "routes:\n  unknownRoutes: 1\n", "unknown field `unknownRoutes`"),
        ("cors", "cors:\n  unknownCors: 1\n", "unknown field `unknownCors`"),
        ("ui", "ui:\n  unknownUi: 1\n", "unknown field `unknownUi`"),
        ("resolver", "resolver:\n  unknownResolver: 1\n", "unknown field `unknownResolver`"),
        (
            "hosted registry",
            "registries:\n  h:\n    type: hosted\n    unknownReg: 1\n",
            "a registry with type hosted, upstream, or router",
        ),
        (
            "upstream registry",
            "registries:\n  u:\n    type: upstream\n    url: https://x/\n    public: true\n    unknownUpstream: 1\n",
            "a registry with type hosted, upstream, or router",
        ),
        (
            "router registry",
            "registries:\n  r:\n    type: router\n    unknownRouter: 1\n",
            "a registry with type hosted, upstream, or router",
        ),
    ];
    for (name, yaml, expected_snippet) in cases {
        let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
            .expect_err(&format!("expected {name} to reject unknown field"));
        assert!(
            err.to_string().contains(expected_snippet),
            "expected {name} error to contain {expected_snippet:?}, got {err}",
        );
    }
}

#[test]
fn config_accepts_camel_case_and_snake_case_aliases() {
    let yaml = "\
storage: ./storage
packument_ttl: 42s
request_timeout: 55s
trusted_proxies: [127.0.0.1]
trusted_proxy_header: x-real-ip
cors:
  allowed_origins: ['https://example.com']
routes:
  allowed_private_networks: ['10.0.0.0/8']
auth:
  htpasswd:
    max_users: 10
backend:
  libsql:
    url: libsql://test.db
    auth_token: secret
    replica_path: ./replica.db
    sync_interval: 15s
registries:
  npm:
    type: hosted
    teams_managed_by: api
    rules_managed_by: api
  remote:
    type: upstream
    url: https://registry.npmjs.org/
    access: [$all]
    max_fails: 4
    fail_timeout: 25s
    max_age: 12m
    auth:
      type: bearer
      token: secret-token
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.http.packument_ttl, std::time::Duration::from_secs(42));
    assert_eq!(config.http.request_timeout, Some(std::time::Duration::from_secs(55)));
    assert_eq!(config.http.trusted_proxies.len(), 1);
    assert_eq!(config.http.trusted_proxy_header, crate::TrustedProxyHeader::XRealIp);
    assert_eq!(config.identity.auth.htpasswd.max_users, super::super::MaxUsers::Limited(10));
    match &config.identity.backend {
        crate::BackendConfig::Libsql(settings) => {
            assert_eq!(settings.sync_interval_secs, Some(15));
            assert_eq!(settings.auth_token.as_deref(), Some("secret"));
        }
        other => panic!("expected libsql, got {other:?}"),
    }
    let hosted = &config.routing.hosted["npm"];
    assert_eq!(hosted.teams_managed_by, crate::Management::Api);
    assert_eq!(hosted.rules_managed_by, crate::Management::Api);
    let upstream = &config.routing.upstreams["remote"];
    assert_eq!(upstream.requests.max_fails, 4);
    assert_eq!(upstream.requests.fail_timeout, std::time::Duration::from_secs(25));
    assert_eq!(upstream.maxage, Some(std::time::Duration::from_mins(12)));
}

#[test]
fn secret_bearing_fields_reject_unresolved_env_vars() {
    let cases = [
        ("secret", "secret: ${PNPR_TEST_UNRESOLVED_VAR_123}\n"),
        ("secrets.cache", "secrets:\n  cache: ${PNPR_TEST_UNRESOLVED_VAR_123}\n"),
        ("secrets.oci", "secrets:\n  oci: ${PNPR_TEST_UNRESOLVED_VAR_123}\n"),
        (
            "s3.secretAccessKey",
            "s3:\n  bucket: b\n  secretAccessKey: ${PNPR_TEST_UNRESOLVED_VAR_123}\n",
        ),
        ("auth.scim.token", "auth:\n  scim:\n    token: ${PNPR_TEST_UNRESOLVED_VAR_123}\n"),
        (
            "auth.oidc.clientSecret",
            "auth:\n  oidc:\n    - name: idp\n      issuer: https://idp.test\n      audience: aud\n      login:\n        clientSecret: ${PNPR_TEST_UNRESOLVED_VAR_123}\n        users: []\n",
        ),
        (
            "backend.libsql.authToken",
            "backend:\n  libsql:\n    url: libsql://x\n    authToken: ${PNPR_TEST_UNRESOLVED_VAR_123}\n",
        ),
        (
            "upstream.auth.token",
            "registries:\n  ext:\n    type: upstream\n    url: https://registry.npmjs.org/\n    access: [$all]\n    auth:\n      type: bearer\n      token: ${PNPR_TEST_UNRESOLVED_VAR_123}\n",
        ),
        (
            "grouped.upstream.auth.token",
            "registries:\n  npm:\n    ext:\n      type: upstream\n      url: https://registry.npmjs.org/\n      access: [$all]\n      auth:\n        type: bearer\n        token: ${PNPR_TEST_UNRESOLVED_VAR_123}\n",
        ),
        (
            "upstream.headers.authorization",
            "registries:\n  ext:\n    type: upstream\n    url: https://registry.npmjs.org/\n    access: [$all]\n    headers:\n      Authorization: Bearer ${PNPR_TEST_UNRESOLVED_VAR_123}\n",
        ),
        (
            "grouped.nested.named.type.auth.token",
            "registries:\n  npm:\n    type:\n      type: upstream\n      url: https://registry.npmjs.org/\n      access: [$all]\n      auth:\n        type: bearer\n        token: ${PNPR_TEST_UNRESOLVED_VAR_123}\n",
        ),
        (
            "grouped.nested.named.url.auth.token",
            "registries:\n  npm:\n    url:\n      type: upstream\n      url: https://registry.npmjs.org/\n      access: [$all]\n      auth:\n        type: bearer\n        token: ${PNPR_TEST_UNRESOLVED_VAR_123}\n",
        ),
    ];

    for (field, yaml) in cases {
        let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap_err();
        match err {
            RegistryError::InvalidConfig { reason } => {
                assert!(
                    reason.contains("references unset environment variable"),
                    "expected unset env var error for {field}, got: {reason}",
                );
            }
            other => panic!("expected InvalidConfig for {field}, got {other:?}"),
        }
    }
}

#[test]
fn secret_bearing_fields_accept_resolved_fallback_env_vars() {
    let yaml = "\
secret: ${PNPR_TEST_UNSET_KEY:-my-fallback-secret-at-least-32-bytes}
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(config.has_configured_secret);
    assert_eq!(config.resolution_cache_secret.as_ref(), b"my-fallback-secret-at-least-32-bytes");
}

#[test]
fn independent_secrets_configuration_and_derivation() {
    let yaml = "\
secrets:
  cache: my-explicit-cache-secret-32-bytes
  oci: my-explicit-oci-secret-32-bytes!!
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(config.has_configured_secret);
    assert_eq!(config.cache_secret.as_ref(), b"my-explicit-cache-secret-32-bytes");
    assert_eq!(config.oci_secret.as_ref(), b"my-explicit-oci-secret-32-bytes!!");

    let master_yaml = "secret: master-secret-at-least-32-bytes-long";
    let master_config =
        Config::from_yaml_str(master_yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(master_config.has_configured_secret);
    assert_eq!(master_config.cache_secret.as_ref(), b"master-secret-at-least-32-bytes-long");
    assert_ne!(master_config.oci_secret.as_ref(), master_config.cache_secret.as_ref());
    assert_eq!(master_config.oci_secret.len(), 32);
}

#[test]
fn oci_only_secret_does_not_flag_configured_cache_secret() {
    let yaml = "\
secrets:
  oci: my-explicit-oci-secret-32-bytes!!
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(!config.has_configured_secret);
    assert_eq!(config.oci_secret.as_ref(), b"my-explicit-oci-secret-32-bytes!!");
    assert_eq!(config.cache_secret.len(), 32);
}
