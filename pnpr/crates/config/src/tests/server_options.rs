use super::{
    Config, Identity, LogFormat, LogLevel, Path, PathBuf, RegistryError, listen, parse_log_yaml,
};

#[test]
fn compiler_cache_policies_distinguish_readers_and_publishers() {
    let config = Config::from_yaml_str(
        "artifacts:\n  enabled: true\n  compilerCaches:\n    acme:\n      access: [ci, developer]\n      publish: ci\n    disabled:\n      access: []\n      publish: []\n",
        Path::new("/config"), listen(), None,
    ).unwrap();
    let policy = &config.artifacts.compiler_caches["acme"];
    assert!(policy.access.allows(&Identity::user("developer")), "developer must be able to read");
    assert!(!policy.publish.allows(&Identity::user("developer")), "developer must not publish");
    assert!(policy.publish.allows(&Identity::user("ci")), "CI must be able to publish");
    assert!(!policy.access.allows(&Identity::Anonymous), "anonymous reads must not be granted");
    assert!(
        config.artifacts.compiler_caches["disabled"].access.is_empty(),
        "empty access must deny reads",
    );
}

#[test]
fn compiler_cache_policies_reject_ambiguous_or_incomplete_declarations() {
    for declaration in [
        "    '../acme': { access: ci, publish: ci }",
        "    acme: { access: ci }",
        "    acme: { access: 'ci developer', publish: ci }",
        "    acme: { access: ci, publish: 'team:missing' }",
        "    acme: { access: ci, publish: ci, unexpected: true }",
    ] {
        let yaml = format!("artifacts:\n  enabled: true\n  compilerCaches:\n{declaration}\n");
        let result = Config::from_yaml_str(&yaml, Path::new("/config"), listen(), None);
        assert!(result.is_err(), "accepted {declaration:?}");
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
fn explicit_cache_key_overrides_the_default() {
    let yaml = "storage: /var/lib/pnpr\ncache: /scratch/pnpr\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage, PathBuf::from("/var/lib/pnpr"));
    assert_eq!(config.cache_storage, PathBuf::from("/scratch/pnpr"));
}

#[test]
fn relative_cache_key_is_resolved_against_base_dir() {
    let yaml = "storage: ./store\ncache: ./cache\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.cache_storage, PathBuf::from("/etc/pnpr/./cache"));
}

#[test]
fn osv_config_defaults_off_and_resolves_relative_path() {
    let defaulted =
        Config::from_yaml_str("upstreams: {}\n", Path::new("/etc/pnpr"), listen(), None).unwrap();
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
upstreams: {}
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
upstreams: {}
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
    // `type: file` parses (verdaccio compatibility) but is not a
    // sink the server implements, so `sink_is_supported` is false
    // and the binary warns at startup. Format/level still apply.
    let yaml = "\
storage: ./s
upstreams: {}
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
upstreams: {}
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
fn log_legacy_plural_list_is_ignored() {
    // Verdaccio 4/5 used `logs:` as a list. We only honor the
    // verdaccio-6 `log:` (singular) shape, so the older spelling
    // is silently dropped and defaults apply.
    let yaml = "\
storage: ./s
upstreams: {}
logs:
  - type: stdout
    format: json
    level: error
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.logs.format, LogFormat::Pretty);
    assert_eq!(config.logs.level, LogLevel::Info);
}

#[test]
fn log_missing_fields_fall_back_to_defaults() {
    // Only `type:` is given. Format and level default individually.
    let yaml = "\
storage: ./s
upstreams: {}
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
