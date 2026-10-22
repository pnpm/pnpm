use super::{
    ArtifactQuota, Config, Identity, LogFormat, LogLevel, Path, PathBuf, RegistryError, listen,
    parse_log_yaml,
};

#[test]
fn org_policies_distinguish_readers_and_publishers() {
    let config = Config::from_yaml_str(
        "artifacts:\n  enabled: true\n  orgs:\n    acme:\n      access: [ci, developer]\n      publish: ci\n    disabled:\n      access: []\n      publish: []\n",
        Path::new("/config"), listen(), None,
    ).unwrap();
    let policy = &config.features.artifacts.orgs["acme"];
    assert!(policy.access.allows(&Identity::user("developer")), "developer must be able to read");
    assert!(!policy.publish.allows(&Identity::user("developer")), "developer must not publish");
    assert!(policy.publish.allows(&Identity::user("ci")), "CI must be able to publish");
    assert!(!policy.access.allows(&Identity::Anonymous), "anonymous reads must not be granted");
    assert!(
        config.features.artifacts.orgs["disabled"].access.is_empty(),
        "empty access must deny reads",
    );
}

#[test]
fn org_policies_reject_ambiguous_or_incomplete_declarations() {
    for declaration in [
        "    '../acme': { access: ci, publish: ci }",
        "    acme: { access: ci }",
        "    acme: { access: 'ci developer', publish: ci }",
        "    acme: { access: ci, publish: 'team:missing' }",
        "    acme: { access: ci, publish: ci, unexpected: true }",
    ] {
        let yaml = format!("artifacts:\n  enabled: true\n  orgs:\n{declaration}\n");
        let result = Config::from_yaml_str(&yaml, Path::new("/config"), listen(), None);
        assert!(result.is_err(), "accepted {declaration:?}");
    }
}

#[test]
fn artifact_quota_is_configured_in_gib() {
    let config = Config::from_yaml_str("", Path::new("/config"), listen(), None).unwrap();
    assert_eq!(config.features.artifacts.quota, ArtifactQuota::default());

    let yaml = "artifacts:\n  quota:\n    ownerGiB: 50\n    totalGiB: 500\n";
    let config = Config::from_yaml_str(yaml, Path::new("/config"), listen(), None).unwrap();
    let gib = 1024 * 1024 * 1024;
    assert_eq!(
        config.features.artifacts.quota,
        ArtifactQuota { owner: 50 * gib, total: 500 * gib },
    );

    for quota in ["ownerGiB: 0", "ownerGiB: 20\n    totalGiB: 10", "perOwner: 1"] {
        let yaml = format!("artifacts:\n  quota:\n    {quota}\n");
        let result = Config::from_yaml_str(&yaml, Path::new("/config"), listen(), None);
        assert!(result.is_err(), "accepted {quota:?}");
    }
}

#[test]
fn rejects_non_origin_cors_urls() {
    for origin in ["*", "null", "ftp://example.test", "https://example.test/path"] {
        let yaml = format!("cors:\n  allowedOrigins: [{origin:?}]\n");
        let err = Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None).unwrap_err();
        assert!(
            matches!(err, RegistryError::InvalidConfig { reason } if reason.contains("CORS allowed origin")),
            "expected an InvalidConfig for {origin:?}",
        );
    }
}

#[test]
fn ui_defaults_to_enabled_and_resolves_its_dir_against_base_dir() {
    let config = Config::from_yaml_str("", Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert!(config.http.ui.enabled);
    assert_eq!(config.http.ui.dir, None);

    let yaml = "ui:\n  enabled: false\n  dir: ./ui\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert!(!config.http.ui.enabled);
    assert_eq!(config.http.ui.dir, Some(PathBuf::from("/etc/pnpr/ui")));

    let err = Config::from_yaml_str("ui:\n  path: ./ui\n", Path::new("/x"), listen(), None);
    assert!(err.is_err(), "accepted an unknown ui key");
}

#[test]
fn explicit_cache_key_overrides_the_default() {
    let yaml = "storage: /var/lib/pnpr\ncache: /scratch/pnpr\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage.hosted_dir, PathBuf::from("/var/lib/pnpr"));
    assert_eq!(config.storage.cache_dir, PathBuf::from("/scratch/pnpr"));
}

#[test]
fn relative_cache_key_is_resolved_against_base_dir() {
    let yaml = "storage: ./store\ncache: ./cache\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage.cache_dir, PathBuf::from("/etc/pnpr/./cache"));
}

#[test]
fn osv_config_defaults_off_and_resolves_relative_path() {
    let defaulted =
        Config::from_yaml_str("storage: /etc/pnpr\n", Path::new("/etc/pnpr"), listen(), None)
            .unwrap();
    assert!(!defaulted.osv.enabled);
    assert_eq!(defaulted.osv.path, None);

    let yaml = "osv:\n  enabled: true\n  path: ./osv/npm/all.zip\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert!(config.osv.enabled);
    assert_eq!(config.osv.path, Some(PathBuf::from("/etc/pnpr/./osv/npm/all.zip")));
}

#[test]
fn sql_backend_rejects_zero_startup_timeout() {
    let yaml = "\
storage: /var/lib/pnpr
backend:
  mysql:
    url: mysql://pnpr:secret@db.example/pnpr
    startupTimeout: 0
";
    let err = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None)
        .expect_err("zero startup timeout must not be accepted");
    assert!(
        matches!(err, RegistryError::InvalidConfig { ref reason } if reason.contains("backend.mysql.startupTimeout")),
        "expected an InvalidConfig naming backend.mysql.startupTimeout, got {err:?}",
    );
}

#[test]
fn sql_backend_rejects_zero_timeout() {
    let yaml = "\
storage: /var/lib/pnpr
backend:
  postgres:
    url: postgres://pnpr:secret@db.example/pnpr
    timeout: 0
";
    let err = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None)
        .expect_err("zero timeout must not be accepted");
    assert!(
        matches!(err, RegistryError::InvalidConfig { ref reason } if reason.contains("backend.postgres.timeout")),
        "expected an InvalidConfig naming backend.postgres.timeout, got {err:?}",
    );
}

#[test]
fn log_unsupported_sink_type_is_recorded_but_flagged_unsupported() {
    // `type: file` parses but is not a sink the server implements, so
    // `sink_is_supported` is false and the binary warns at startup.
    // Format/level still apply.
    let yaml = "\
storage: ./s
log:
  type: file
  format: json
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.logs.sink, "file");
    assert!(!config.logs.sink_is_supported());
    assert_eq!(config.logs.format, LogFormat::Json);
}

#[test]
fn log_pretty_and_level_picked_from_singular_block() {
    let yaml = "\
storage: ./s
log:
  type: stdout
  format: pretty
  level: warn
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.logs.format, LogFormat::Pretty);
    assert_eq!(config.logs.level, LogLevel::Warn);
}

#[test]
fn log_legacy_plural_list_is_rejected() {
    let yaml = "\
storage: ./s
logs:
  - type: stdout
    format: json
    level: error
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap_err();
    assert!(err.to_string().contains("unknown field `logs`"));
}

#[test]
fn log_missing_fields_fall_back_to_defaults() {
    // Only `type:` is given. Format and level default individually.
    let yaml = "\
storage: ./s
log:
  type: stdout
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.logs.format, LogFormat::Pretty);
    assert_eq!(config.logs.level, LogLevel::Info);
}

#[test]
fn log_level_filter_directives_are_valid() {
    // Each LogLevel must map to a directive string that
    // `EnvFilter::new` accepts at runtime — guards against typos.
    for level in [
        LogLevel::Trace,
        LogLevel::Debug,
        LogLevel::Http,
        LogLevel::Info,
        LogLevel::Warn,
        LogLevel::Error,
    ] {
        let directive = level.as_filter_directive();
        tracing_subscriber::EnvFilter::try_new(directive)
            .unwrap_or_else(|err| panic!("{level:?} -> `{directive}`: {err}"));
    }
}

#[test]
fn log_format_accepts_each_known_variant() {
    assert_eq!(parse_log_yaml::<LogFormat>("pretty").unwrap(), LogFormat::Pretty);
    assert_eq!(parse_log_yaml::<LogFormat>("json").unwrap(), LogFormat::Json);
}

#[test]
fn log_format_rejects_unknown_variant() {
    // `format: xml` (or anything else) should fail parsing
    // rather than silently fall back. Matches verdaccio: an
    // unknown enum value is a typo, not a request for a default.
    let err = parse_log_yaml::<LogFormat>("xml").unwrap_err();
    assert!(err.contains("xml") || err.to_lowercase().contains("unknown"));
}

#[test]
fn log_format_is_case_sensitive() {
    // `rename_all = "lowercase"` means we accept only lowercase
    // tokens; pino is case-sensitive too.
    assert!(parse_log_yaml::<LogFormat>("Pretty").is_err());
    assert!(parse_log_yaml::<LogFormat>("JSON").is_err());
}

#[test]
fn log_level_accepts_each_known_variant() {
    let pairs: &[(&str, LogLevel)] = &[
        ("trace", LogLevel::Trace),
        ("debug", LogLevel::Debug),
        ("http", LogLevel::Http),
        ("info", LogLevel::Info),
        ("warn", LogLevel::Warn),
        ("error", LogLevel::Error),
    ];
    for (yaml, expected) in pairs {
        let parsed: LogLevel = parse_log_yaml(yaml).unwrap();
        assert_eq!(parsed, *expected, "{yaml}");
    }
}

#[test]
fn log_level_rejects_unknown_variant() {
    // `fatal` (pino has it) and `silly` (npm's logger had it)
    // are not in our set — we want a hard error, not a silent
    // fallback.
    assert!(parse_log_yaml::<LogLevel>("fatal").is_err());
    assert!(parse_log_yaml::<LogLevel>("silly").is_err());
    assert!(parse_log_yaml::<LogLevel>("verbose").is_err());
}

#[test]
fn a_scim_token_must_be_long_and_stays_out_of_debug_output() {
    let yaml = |token: &str| format!("storage: ./s\nauth:\n  scim:\n    token: {token}\n");
    let err = Config::from_yaml_str(&yaml("short"), Path::new("/x"), listen(), None)
        .expect_err("a short SCIM token must be rejected");
    assert!(
        matches!(&err, RegistryError::InvalidConfig { reason } if reason.contains("auth.scim.token")),
        "{err}",
    );
    let token = "a".repeat(crate::ScimConfig::MIN_TOKEN_LEN);
    let config = Config::from_yaml_str(&yaml(&token), Path::new("/x"), listen(), None).unwrap();
    let scim = config.identity.auth.scim.as_ref().expect("auth.scim is set");
    assert_eq!(scim.token, token);
    assert!(!format!("{scim:?}").contains(&token));
}

#[test]
fn the_retired_per_surface_policies_are_refused() {
    for yaml in [
        "artifacts:\n  compilerCaches:\n    acme: { access: ci, publish: ci }\n",
        "pipeline:\n  workspaces:\n    app: { access: ci, publish: ci }\n",
    ] {
        let result = Config::from_yaml_str(yaml, Path::new("/config"), listen(), None);
        assert!(result.is_err(), "accepted {yaml:?}");
    }
}

#[test]
fn sql_backend_and_libsql_settings_redact_passwords_and_tokens_in_debug() {
    let sql = crate::SqlBackendSettings {
        url: "postgres://user:supersecret@localhost:5432/db?password=querysecret&other=val"
            .to_string(),
        max_connections: None,
        timeout: std::time::Duration::from_secs(30),
        startup_timeout: std::time::Duration::from_mins(5),
    };
    let rendered_sql = format!("{sql:?}");
    assert!(!rendered_sql.contains("supersecret"));
    assert!(!rendered_sql.contains("querysecret"));
    assert!(rendered_sql.contains("redacted"));
    assert!(rendered_sql.contains("other=val"));

    let libsql = crate::LibsqlSettings {
        url: "https://libsql.example.com?authToken=mysecrettoken".to_string(),
        auth_token: Some("secret_auth_token".to_string()),
        replica_path: None,
        sync_interval_secs: None,
        max_connections: None,
    };
    let rendered_libsql = format!("{libsql:?}");
    assert!(!rendered_libsql.contains("mysecrettoken"));
    assert!(!rendered_libsql.contains("secret_auth_token"));
    assert!(rendered_libsql.contains("redacted"));

    let token_url_sql = crate::SqlBackendSettings {
        url: "postgres://TOKEN@localhost:5432/db".to_string(),
        max_connections: None,
        timeout: std::time::Duration::from_secs(30),
        startup_timeout: std::time::Duration::from_mins(5),
    };
    let rendered_token = format!("{token_url_sql:?}");
    assert!(!rendered_token.contains("TOKEN"));
    assert!(rendered_token.contains("redacted"));

    let punctuated_sql = crate::SqlBackendSettings {
        url: "postgres://user:p)ass@db.example/pnpr".to_string(),
        max_connections: None,
        timeout: std::time::Duration::from_secs(30),
        startup_timeout: std::time::Duration::from_mins(5),
    };
    let rendered_punctuated = format!("{punctuated_sql:?}");
    assert!(!rendered_punctuated.contains("p)ass"));
    assert!(rendered_punctuated.contains("redacted"));
}

#[test]
fn trusted_proxies_parses_cidrs_and_loopback() {
    let yaml = "\
storage: ./s
trustedProxies:
  - loopback
  - 10.0.0.0/8
  - 192.168.1.1
trustedProxyHeader: forwarded
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.http.trusted_proxies.len(), 4);
    assert_eq!(config.http.trusted_proxy_header, super::super::TrustedProxyHeader::Forwarded);
}

#[test]
fn request_timeout_defaults_and_overrides() {
    let base_config =
        Config::from_yaml_str("storage: ./s\n", Path::new("/x"), listen(), None).unwrap();
    assert_eq!(base_config.http.request_timeout, Some(crate::HttpConfig::DEFAULT_REQUEST_TIMEOUT));

    let custom_yaml = "storage: ./s\nrequestTimeout: 15s\n";
    let custom_config =
        Config::from_yaml_str(custom_yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(custom_config.http.request_timeout, Some(std::time::Duration::from_secs(15)));

    let disabled_yaml = "storage: ./s\nrequestTimeout: 0\n";
    let disabled_config =
        Config::from_yaml_str(disabled_yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(disabled_config.http.request_timeout, None);

    let invalid_yaml = "storage: ./s\nrequestTimeout: invalid_val\n";
    let err = Config::from_yaml_str(invalid_yaml, Path::new("/x"), listen(), None).unwrap_err();
    assert!(
        matches!(err, RegistryError::InvalidConfig { ref reason } if reason.contains("requestTimeout")),
        "expected InvalidConfig with requestTimeout, got {err:?}",
    );
}
