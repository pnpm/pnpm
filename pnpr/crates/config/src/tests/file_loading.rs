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
    assert!(config.registry.enabled);
    assert!(!config.resolver.enabled);
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
    let upstream = &config.upstreams["npmjs"];
    assert_eq!(upstream.maxage, Some(Duration::from_mins(10)));
    assert_eq!(upstream.timeout, Duration::from_secs(45));
}

#[test]
fn from_yaml_str_tolerates_unresolved_env_var_references() {
    let yaml = r"
storage: ${PNPR_UNSET_VAR_FOR_TEST}./store
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect("an unresolved ${VAR} is replaced with empty, not an error");
    assert!(config.storage.ends_with("store"));
}

#[test]
fn from_default_yaml_parses_bundled_file() {
    use pnpr_registry::{ConcreteKind, Resolved};
    let config = Config::from_default_yaml(Path::new("/tmp"), listen(), None);
    assert!(config.upstreams.contains_key("npmjs"));
    assert_eq!(config.upstreams["npmjs"].url, "https://registry.npmjs.org/");
    assert_eq!(config.auth.htpasswd.max_users, super::super::MaxUsers::Disabled);
    // The bundled file routes fixture scopes, the fixture packages living in
    // real npm scopes, and test-published names to the local hosted org, and
    // everything else — including the rest of those real scopes — to npmjs.
    for local in ["@pnpm.e2e/foo", "@pnpm/y", "test-publish-tarball", "project-100"] {
        assert_eq!(
            config.registries.resolve_default(Ecosystem::Npm, local),
            Resolved::Concrete { registry: "local", kind: ConcreteKind::Hosted },
            "{local} must be hosted",
        );
    }
    for upstream in ["react", "lodash", "test-exclude", "@pnpm/error"] {
        assert_eq!(
            config.registries.resolve_default(Ecosystem::Npm, upstream),
            Resolved::Concrete { registry: "npmjs", kind: ConcreteKind::Upstream },
            "{upstream} must proxy npm",
        );
    }
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
fn from_yaml_str_ignores_unknown_sections() {
    // Sections we don't implement (`auth`, `web`, `plugins`, etc.)
    // must parse silently so existing config files work untouched.
    let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
web:
  enable: false
plugins: ../node_modules
secret: a-sufficiently-long-secret-value
upstreams:
  npmjs:
    url: https://registry.npmjs.org/
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    // The unimplemented sections parse silently and the config is usable.
    assert_eq!(config.auth.htpasswd.max_users, super::super::MaxUsers::Disabled);
}

#[test]
fn from_yaml_str_public_url_defaults_to_listen_when_none_passed() {
    let yaml = "storage: ./s\n";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.public_url, format!("http://{}", listen()));
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
    assert_eq!(config.public_url, "http://override.test");
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
    assert_eq!(config.storage, dir.path().join("./store"));
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
upstreams: {}
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
upstreams:
  npmjs: {{ url: https://registry.npmjs.org/ }}
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
    assert_eq!(config.storage, storage);
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
upstreams: {}
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
    assert_eq!(config.route_policy.public.len(), 2);
    assert_eq!(
        config.route_policy.public[0].registry.as_deref(),
        Some("https://registry.npmjs.org/"),
    );
    assert_eq!(config.route_policy.public[0].package.as_deref(), Some("@babel/*"));
    assert_eq!(config.route_policy.public[1].registry, None);
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
