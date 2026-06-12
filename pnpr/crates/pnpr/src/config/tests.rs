use super::{
    BackendConfig, Config, ConfigSource, DEFAULT_CONFIG_YAML, HostedStoreConfig, LogFormat,
    LogLevel, TokenEnv, UplinkAuthFile, UplinkAuthType, UplinkFile, config_file_in,
    pattern_matches, resolve_relative, resolve_uplink,
};
use crate::{error::RegistryError, policy::Identity};
use indexmap::IndexMap;
use pacquet_env_replace::EnvVar;
use reqwest::header::AUTHORIZATION;
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
};

/// Test [`EnvVar`] provider with a fixed set of variables, so
/// `token_env` resolution can be exercised without touching the real
/// process environment.
struct FakeEnv;

impl EnvVar for FakeEnv {
    fn var(name: &str) -> Option<String> {
        match name {
            "NPM_TOKEN" => Some("default-env-token".to_string()),
            "CUSTOM_TOKEN" => Some("custom-env-token".to_string()),
            "EMPTY_TOKEN" => Some(String::new()),
            _ => None,
        }
    }
}

fn uplink_file(auth: Option<UplinkAuthFile>, headers: IndexMap<String, String>) -> UplinkFile {
    UplinkFile { url: "https://upstream.test/".to_string(), auth, headers }
}

fn auth_header(uplink: &super::UplinkConfig) -> Option<&str> {
    uplink.headers.get(AUTHORIZATION).map(|value| value.to_str().unwrap())
}

#[test]
fn uplink_bearer_token_becomes_bearer_authorization() {
    let auth = UplinkAuthFile {
        r#type: UplinkAuthType::Bearer,
        token: Some("abc123".to_string()),
        token_env: None,
    };
    let uplink = resolve_uplink::<FakeEnv>("npmjs", uplink_file(Some(auth), IndexMap::new()))
        .expect("bearer token resolves");
    assert_eq!(auth_header(&uplink), Some("Bearer abc123"));
}

#[test]
fn uplink_basic_token_becomes_basic_authorization_verbatim() {
    let auth = UplinkAuthFile {
        r#type: UplinkAuthType::Basic,
        token: Some("dXNlcjpwYXNz".to_string()),
        token_env: None,
    };
    let uplink = resolve_uplink::<FakeEnv>("priv", uplink_file(Some(auth), IndexMap::new()))
        .expect("basic token resolves");
    assert_eq!(auth_header(&uplink), Some("Basic dXNlcjpwYXNz"));
}

#[test]
fn uplink_token_env_true_reads_npm_token() {
    let auth = UplinkAuthFile {
        r#type: UplinkAuthType::Bearer,
        token: None,
        token_env: Some(TokenEnv::Flag(true)),
    };
    let uplink = resolve_uplink::<FakeEnv>("npmjs", uplink_file(Some(auth), IndexMap::new()))
        .expect("token_env: true reads NPM_TOKEN");
    assert_eq!(auth_header(&uplink), Some("Bearer default-env-token"));
}

#[test]
fn uplink_token_env_named_reads_that_var() {
    let auth = UplinkAuthFile {
        r#type: UplinkAuthType::Bearer,
        token: None,
        token_env: Some(TokenEnv::Named("CUSTOM_TOKEN".to_string())),
    };
    let uplink = resolve_uplink::<FakeEnv>("npmjs", uplink_file(Some(auth), IndexMap::new()))
        .expect("named token_env reads that var");
    assert_eq!(auth_header(&uplink), Some("Bearer custom-env-token"));
}

#[test]
fn uplink_literal_token_beats_token_env() {
    let auth = UplinkAuthFile {
        r#type: UplinkAuthType::Bearer,
        token: Some("literal".to_string()),
        token_env: Some(TokenEnv::Named("CUSTOM_TOKEN".to_string())),
    };
    let uplink = resolve_uplink::<FakeEnv>("npmjs", uplink_file(Some(auth), IndexMap::new()))
        .expect("literal token wins");
    assert_eq!(auth_header(&uplink), Some("Bearer literal"));
}

#[test]
fn uplink_custom_headers_are_forwarded() {
    let headers = IndexMap::from_iter([("x-custom".to_string(), "value".to_string())]);
    let uplink = resolve_uplink::<FakeEnv>("npmjs", uplink_file(None, headers))
        .expect("custom headers resolve");
    assert_eq!(uplink.headers.get("x-custom").unwrap().to_str().unwrap(), "value");
    assert!(auth_header(&uplink).is_none());
}

#[test]
fn uplink_custom_authorization_header_overrides_auth_block() {
    let auth = UplinkAuthFile {
        r#type: UplinkAuthType::Bearer,
        token: Some("from-auth".to_string()),
        token_env: None,
    };
    let headers =
        IndexMap::from_iter([("authorization".to_string(), "Basic override".to_string())]);
    let uplink = resolve_uplink::<FakeEnv>("npmjs", uplink_file(Some(auth), headers))
        .expect("custom header overrides auth-derived one");
    assert_eq!(auth_header(&uplink), Some("Basic override"));
}

#[test]
fn uplink_auth_without_resolvable_token_is_a_config_error() {
    let auth = UplinkAuthFile {
        r#type: UplinkAuthType::Bearer,
        token: None,
        token_env: Some(TokenEnv::Named("UNSET_VAR".to_string())),
    };
    let err = resolve_uplink::<FakeEnv>("npmjs", uplink_file(Some(auth), IndexMap::new()))
        .expect_err("missing token must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn uplink_auth_with_empty_literal_token_is_a_config_error() {
    let auth = UplinkAuthFile {
        r#type: UplinkAuthType::Bearer,
        token: Some(String::new()),
        token_env: None,
    };
    let err = resolve_uplink::<FakeEnv>("npmjs", uplink_file(Some(auth), IndexMap::new()))
        .expect_err("an empty token must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn uplink_auth_with_empty_env_token_is_a_config_error() {
    let auth = UplinkAuthFile {
        r#type: UplinkAuthType::Bearer,
        token: None,
        token_env: Some(TokenEnv::Named("EMPTY_TOKEN".to_string())),
    };
    let err = resolve_uplink::<FakeEnv>("npmjs", uplink_file(Some(auth), IndexMap::new()))
        .expect_err("an empty env token must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn uplink_token_env_false_resolves_no_token_and_is_a_config_error() {
    let auth = UplinkAuthFile {
        r#type: UplinkAuthType::Bearer,
        token: None,
        token_env: Some(TokenEnv::Flag(false)),
    };
    let err = resolve_uplink::<FakeEnv>("npmjs", uplink_file(Some(auth), IndexMap::new()))
        .expect_err("token_env: false reads nothing, so an auth block must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn uplink_auth_token_with_control_char_is_a_config_error() {
    let auth = UplinkAuthFile {
        r#type: UplinkAuthType::Bearer,
        token: Some("bad\ntoken".to_string()),
        token_env: None,
    };
    let err = resolve_uplink::<FakeEnv>("npmjs", uplink_file(Some(auth), IndexMap::new()))
        .expect_err("a token that is not a valid header value must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn uplink_invalid_custom_header_name_is_a_config_error() {
    let headers = IndexMap::from_iter([("bad header".to_string(), "value".to_string())]);
    let err = resolve_uplink::<FakeEnv>("npmjs", uplink_file(None, headers))
        .expect_err("a header name with a space must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn uplink_invalid_custom_header_value_is_a_config_error() {
    let headers = IndexMap::from_iter([("x-custom".to_string(), "bad\nvalue".to_string())]);
    let err = resolve_uplink::<FakeEnv>("npmjs", uplink_file(None, headers))
        .expect_err("a header value with a control char must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn from_yaml_str_resolves_uplink_auth_and_headers() {
    let yaml = r"
uplinks:
  npmjs:
    url: https://registry.npmjs.org/
    auth:
      type: bearer
      token: secret-token
    headers:
      X-Org: acme
packages:
  '**':
    proxy: npmjs
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let uplink = &config.uplinks["npmjs"];
    assert_eq!(uplink.headers.get(AUTHORIZATION).unwrap().to_str().unwrap(), "Bearer secret-token");
    assert_eq!(uplink.headers.get("x-org").unwrap().to_str().unwrap(), "acme");
}

#[test]
fn from_yaml_str_tolerates_unresolved_env_var_references() {
    let yaml = r"
storage: ${PNPR_UNSET_VAR_FOR_TEST}./store
packages:
  '**':
    proxy: npmjs
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect("an unresolved ${VAR} is replaced with empty, not an error");
    assert!(config.storage.ends_with("store"));
}

fn user(name: &str) -> Identity {
    Identity::User { username: name.to_string() }
}

fn listen() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
}

#[test]
fn pattern_double_star_matches_anything() {
    assert!(pattern_matches("**", "lodash"));
    assert!(pattern_matches("**", "@foo/bar"));
    assert!(pattern_matches("**", ""));
}

#[test]
fn pattern_any_scope_matches_only_scoped() {
    assert!(pattern_matches("@*/*", "@foo/bar"));
    assert!(pattern_matches("@*/*", "@pnpm.e2e/needs-auth"));
    assert!(!pattern_matches("@*/*", "lodash"));
}

#[test]
fn pattern_specific_scope_matches_only_that_scope() {
    assert!(pattern_matches("@private/*", "@private/anything"));
    assert!(!pattern_matches("@private/*", "@public/anything"));
    assert!(!pattern_matches("@private/*", "private"));
}

#[test]
fn pattern_exact_match() {
    assert!(pattern_matches("foobar", "foobar"));
    assert!(!pattern_matches("foobar", "foobaz"));
    assert!(!pattern_matches("foobar", "@scope/foobar"));
}

#[test]
fn resolve_relative_passes_absolute_paths_through() {
    let absolute = PathBuf::from("/tmp/storage");
    assert_eq!(resolve_relative("/tmp/storage", Path::new("/anywhere")), absolute);
}

#[test]
fn resolve_relative_joins_relative_paths_to_base() {
    assert_eq!(
        resolve_relative("./storage", Path::new("/etc/pnpr")),
        PathBuf::from("/etc/pnpr/./storage"),
    );
}

#[test]
fn proxy_constructor_routes_everything_through_npmjs() {
    let config = Config::proxy(listen(), PathBuf::from("/tmp"));
    let (name, uplink) = config.resolve_uplink("anything").expect("** rule matches");
    assert_eq!(name, "npmjs");
    assert_eq!(uplink.url, "https://registry.npmjs.org");
}

#[test]
fn static_constructor_has_no_uplinks() {
    let config = Config::static_serve(listen(), PathBuf::from("/tmp"));
    assert!(config.uplinks.is_empty());
    assert!(config.packages.is_empty());
    assert!(config.resolve_uplink("anything").is_none());
}

#[test]
fn from_default_yaml_parses_bundled_file() {
    let config = Config::from_default_yaml(Path::new("/tmp"), listen(), None);
    assert!(config.uplinks.contains_key("npmjs"));
    assert_eq!(config.uplinks["npmjs"].url, "https://registry.npmjs.org/");
    // The bundled file routes the catch-all through npmjs.
    let (name, _) = config.resolve_uplink("lodash").expect("** -> npmjs in defaults");
    assert_eq!(name, "npmjs");
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
fn from_yaml_str_storage_is_resolved_relative_to_base_dir() {
    let yaml = "storage: ./store\nuplinks: {}\npackages: {}\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage, PathBuf::from("/etc/pnpr/./store"));
}

#[test]
fn from_yaml_str_absolute_storage_is_left_alone() {
    let yaml = "storage: /var/lib/pnpr\nuplinks: {}\npackages: {}\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage, PathBuf::from("/var/lib/pnpr"));
}

#[test]
fn cache_storage_defaults_to_subdir_of_storage() {
    let yaml = "storage: /var/lib/pnpr\nuplinks: {}\npackages: {}\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.cache_storage, PathBuf::from("/var/lib/pnpr/.pnpr-cache"));
}

#[test]
fn explicit_cache_key_overrides_the_default() {
    let yaml = "storage: /var/lib/pnpr\ncache: /scratch/pnpr\nuplinks: {}\npackages: {}\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage, PathBuf::from("/var/lib/pnpr"));
    assert_eq!(config.cache_storage, PathBuf::from("/scratch/pnpr"));
}

#[test]
fn relative_cache_key_is_resolved_against_base_dir() {
    let yaml = "storage: ./store\ncache: ./cache\nuplinks: {}\npackages: {}\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.cache_storage, PathBuf::from("/etc/pnpr/./cache"));
}

#[test]
fn hosted_store_defaults_to_fs_without_an_s3_block() {
    let yaml = "storage: /var/lib/pnpr\nuplinks: {}\npackages: {}\n";
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
uplinks: {}
packages: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.hosted_store {
        HostedStoreConfig::S3 { prefix, .. } => assert_eq!(prefix, "packages/"),
        HostedStoreConfig::Fs => panic!("expected an S3 hosted store, got Fs"),
    }
}

#[test]
fn s3_block_without_a_bucket_is_a_config_error() {
    let yaml = "storage: /x\ns3:\n  region: auto\nuplinks: {}\npackages: {}\n";
    assert!(Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).is_err());
}

#[test]
fn backend_defaults_to_local_without_a_block() {
    let yaml = "storage: /var/lib/pnpr\nuplinks: {}\npackages: {}\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert!(matches!(config.backend, BackendConfig::Local));
}

#[test]
fn libsql_backend_block_selects_the_networked_record_store() {
    let yaml = "\
storage: /var/lib/pnpr
backend:
  libsql:
    url: libsql://db.turso.io
    authToken: tok-secret
uplinks: {}
packages: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.backend {
        BackendConfig::Libsql(settings) => {
            assert_eq!(settings.url, "libsql://db.turso.io");
            assert_eq!(settings.auth_token.as_deref(), Some("tok-secret"));
        }
        BackendConfig::Local => panic!("expected a libsql backend, got Local"),
    }
}

#[test]
fn libsql_backend_auth_token_is_optional() {
    let yaml = "\
storage: /var/lib/pnpr
backend:
  libsql:
    url: http://127.0.0.1:8080
uplinks: {}
packages: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.backend {
        BackendConfig::Libsql(settings) => {
            assert!(settings.auth_token.is_none());
            assert!(settings.replica_path.is_none(), "no replica by default");
        }
        BackendConfig::Local => panic!("expected a libsql backend, got Local"),
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
uplinks: {}
packages: {}
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
        BackendConfig::Local => panic!("expected a libsql backend, got Local"),
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
uplinks: {}
packages: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    match config.backend {
        BackendConfig::Libsql(settings) => assert_eq!(
            settings.replica_path.as_deref(),
            Some(Path::new("/var/lib/pnpr/auth-replica.db")),
        ),
        BackendConfig::Local => panic!("expected a libsql backend, got Local"),
    }
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
secret: hunter2
uplinks:
  npmjs:
    url: https://registry.npmjs.org/
packages:
  '**':
    access: $all
    proxy: npmjs
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let (name, uplink) = config.resolve_uplink("anything").expect("** -> npmjs");
    assert_eq!(name, "npmjs");
    assert_eq!(uplink.url, "https://registry.npmjs.org/");
}

#[test]
fn from_yaml_str_packages_evaluated_in_declared_order() {
    // First match wins: `@private/*` should resolve before `**`
    // even though both are declared.
    let yaml = "\
storage: ./s
uplinks:
  mirror: { url: https://mirror.example/ }
  npmjs:  { url: https://registry.npmjs.org/ }
packages:
  '@private/*':
    proxy: mirror
  '**':
    proxy: npmjs
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.resolve_uplink("@private/foo").unwrap().0, "mirror");
    assert_eq!(config.resolve_uplink("lodash").unwrap().0, "npmjs");
}

#[test]
fn from_yaml_str_package_without_proxy_does_not_resolve_an_uplink() {
    // Verdaccio first-match-wins: a pattern entry that matches but
    // has no `proxy:` is storage-only — resolution stops there and
    // returns None instead of falling through to a later catch-all.
    let yaml = "\
storage: ./s
uplinks:
  npmjs: { url: https://registry.npmjs.org/ }
packages:
  '@private/*':
    access: $authenticated
  '**':
    proxy: npmjs
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(config.resolve_uplink("@private/foo").is_none());
    // Unrelated names still fall through to `**` -> `npmjs`.
    assert_eq!(config.resolve_uplink("lodash").unwrap().0, "npmjs");
}

#[test]
fn from_yaml_str_public_url_defaults_to_listen_when_none_passed() {
    let yaml = "storage: ./s\nuplinks: {}\npackages: {}\n";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.public_url, format!("http://{}", listen()));
}

#[test]
fn from_yaml_str_public_url_override_wins() {
    let yaml = "storage: ./s\nuplinks: {}\npackages: {}\n";
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
    std::fs::write(&config_path, "storage: ./store\nuplinks: {}\npackages: {}\n").unwrap();
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
fn auth_block_resolves_htpasswd_relative_to_config_dir() {
    let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
uplinks: {}
packages: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.auth.htpasswd.file.as_deref(), Some(Path::new("/etc/pnpr/./htpasswd")));
    // Tokens default to the htpasswd sibling.
    assert_eq!(config.auth.tokens.file.as_deref(), Some(Path::new("/etc/pnpr/tokens.db")));
}

#[test]
fn auth_block_absent_keeps_in_memory_defaults() {
    let yaml = "storage: ./s\nuplinks: {}\npackages: {}\n";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(config.auth.htpasswd.file.is_none());
    assert!(config.auth.tokens.file.is_none());
    assert_eq!(config.auth.htpasswd.max_users, super::MaxUsers::Unlimited);
}

#[test]
fn auth_tokens_file_explicit_override_wins_over_sibling_default() {
    let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
  tokens:
    file: /var/lib/pnpr/tokens.sqlite
uplinks: {}
packages: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.auth.tokens.file.as_deref(), Some(Path::new("/var/lib/pnpr/tokens.sqlite")));
}

#[test]
fn auth_max_users_negative_one_means_disabled() {
    let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
    max_users: -1
uplinks: {}
packages: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.auth.htpasswd.max_users, super::MaxUsers::Disabled);
}

#[test]
fn auth_max_users_positive_is_a_hard_cap() {
    let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
    max_users: 5
uplinks: {}
packages: {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.auth.htpasswd.max_users, super::MaxUsers::Limited(5));
}

#[test]
fn logs_default_when_yaml_omits_block() {
    let yaml = "storage: ./s\nuplinks: {}\npackages: {}\n";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.logs.format, LogFormat::Pretty);
    assert_eq!(config.logs.level, LogLevel::Info);
    assert_eq!(config.logs.sink, "stdout");
    assert!(config.logs.sink_is_supported());
}

#[test]
fn log_unsupported_sink_type_is_recorded_but_flagged_unsupported() {
    // `type: file` parses (verdaccio compatibility) but is not a
    // sink the server implements, so `sink_is_supported` is false
    // and the binary warns at startup. Format/level still apply.
    let yaml = "\
storage: ./s
uplinks: {}
packages: {}
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
uplinks: {}
packages: {}
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
fn log_json_format_parses() {
    let yaml = "\
storage: ./s
uplinks: {}
packages: {}
log:
  type: stdout
  format: json
  level: debug
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(config.logs.format, LogFormat::Json);
    assert_eq!(config.logs.level, LogLevel::Debug);
}

#[test]
fn log_legacy_plural_list_is_ignored() {
    // Verdaccio 4/5 used `logs:` as a list. We only honor the
    // verdaccio-6 `log:` (singular) shape, so the older spelling
    // is silently dropped and defaults apply.
    let yaml = "\
storage: ./s
uplinks: {}
packages: {}
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
uplinks: {}
packages: {}
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
    std::fs::write(&expected, "storage: ./s\nuplinks: {}\npackages: {}\n").unwrap();
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
uplinks:
  npmjs: {{ url: https://registry.npmjs.org/ }}
packages:
  '**':
    proxy: npmjs
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
    assert_eq!(config.resolve_uplink("lodash").unwrap().0, "npmjs");
}

// ----- LogFormat / LogLevel serde behavior ------------------------------

/// Helper: deserialize a YAML scalar into the requested enum.
/// Lets us assert the variant mapping concisely.
fn parse_log_yaml<Target: serde::de::DeserializeOwned>(yaml: &str) -> Result<Target, String> {
    serde_saphyr::from_str::<Target>(yaml).map_err(|err| err.to_string())
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

// ----- Config::resolve precedence ---------------------------------------

/// Helper: write a config file under a tempdir and hand back the
/// path. Tests use this to populate both the explicit `-c` arg
/// and the auto-discovered default path.
fn write_yaml(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write yaml fixture");
    path
}

const MINIMAL_YAML: &str = "storage: ./s\nuplinks: {}\npackages: {}\n";

#[test]
fn resolve_bundled_when_no_path_supplied() {
    let (config, source) = Config::resolve(None, None, listen(), None).unwrap();
    assert_eq!(source, ConfigSource::Bundled);
    // The bundled config has the `npmjs` uplink + `**` route.
    assert!(config.uplinks.contains_key("npmjs"));
}

#[test]
fn resolve_default_path_when_only_default_supplied() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_yaml(tmp.path(), "config.yaml", MINIMAL_YAML);
    let (_, source) = Config::resolve(None, Some(&path), listen(), None).unwrap();
    assert_eq!(source, ConfigSource::DefaultPath(path));
}

#[test]
fn resolve_cli_when_only_cli_supplied() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_yaml(tmp.path(), "explicit.yml", MINIMAL_YAML);
    let (_, source) = Config::resolve(Some(&path), None, listen(), None).unwrap();
    assert_eq!(source, ConfigSource::Cli(path));
}

#[test]
fn resolve_cli_wins_over_default_path() {
    // Both paths exist. CLI must take priority — the auto-discovered
    // path is a *fallback*, not a merge target.
    //
    // Storage paths are derived from `tmp` so they're absolute on
    // every OS (Windows needs a drive-letter prefix to satisfy
    // `Path::is_absolute()`; a Unix-style `/a` is not absolute
    // there and gets joined to the config file's parent dir).
    let tmp = tempfile::tempdir().unwrap();
    let cli_storage = tmp.path().join("from-cli");
    let default_storage = tmp.path().join("from-default");
    let cli = write_yaml(
        tmp.path(),
        "explicit.yml",
        &format!("storage: {}\nuplinks: {{}}\npackages: {{}}\n", cli_storage.display()),
    );
    let default = write_yaml(
        tmp.path(),
        "default.yml",
        &format!("storage: {}\nuplinks: {{}}\npackages: {{}}\n", default_storage.display()),
    );
    let (config, source) = Config::resolve(Some(&cli), Some(&default), listen(), None).unwrap();
    assert_eq!(source, ConfigSource::Cli(cli));
    // Confirms the *content* came from the CLI file, not the default.
    assert_eq!(config.storage, cli_storage);
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
fn resolve_public_url_override_threads_through() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_yaml(tmp.path(), "config.yaml", MINIMAL_YAML);
    let (config, _) =
        Config::resolve(Some(&path), None, listen(), Some("http://override.test".to_string()))
            .unwrap();
    assert_eq!(config.public_url, "http://override.test");
}

#[test]
fn resolve_bundled_branch_honors_public_url_override() {
    let (config, source) =
        Config::resolve(None, None, listen(), Some("http://from-cli.test".to_string())).unwrap();
    assert_eq!(source, ConfigSource::Bundled);
    assert_eq!(config.public_url, "http://from-cli.test");
}

// ----- serde defaults ---------------------------------------------------

#[test]
fn yaml_with_no_storage_uses_default_storage_string() {
    // `storage:` is absent entirely — `default_storage_string`
    // supplies `"./storage"`, which `resolve_relative` then joins
    // to the config-file's parent dir.
    let yaml = "uplinks: {}\npackages: {}\n";
    let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
    assert_eq!(config.storage, PathBuf::from("/etc/pnpr/./storage"));
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
uplinks: {}
packages: {}
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

// ----- policy wiring from YAML ------------------------------------------

#[test]
fn policies_are_derived_from_packages_block() {
    // The `access` / `publish` tokens in each entry drive the
    // runtime policy — not a hard-coded default set.
    let yaml = "\
storage: ./s
uplinks: {}
packages:
  '@secret/*':
    access: $authenticated
    publish: $authenticated
  '**':
    access: $all
    publish: $authenticated
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let secret = config.policies.for_package("@secret/thing");
    assert!(!secret.access.allows(&Identity::Anonymous));
    assert!(secret.access.allows(&user("alice")));
    let public = config.policies.for_package("lodash");
    assert!(public.access.allows(&Identity::Anonymous));
    assert!(!public.publish.allows(&Identity::Anonymous));
}

#[test]
fn policy_first_matching_rule_wins() {
    // `@secret/*` is declared before the `**` catch-all, so it
    // wins for a scoped package even though both match.
    let yaml = "\
storage: ./s
uplinks: {}
packages:
  '@secret/*':
    access: $authenticated
  '**':
    access: $all
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert!(!config.policies.for_package("@secret/x").access.allows(&Identity::Anonymous));
    assert!(config.policies.for_package("anything").access.allows(&Identity::Anonymous));
}

#[test]
fn policy_missing_access_and_publish_default_to_all_and_authenticated() {
    let yaml = "\
storage: ./s
uplinks: {}
packages:
  'lodash': {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let effective = config.policies.for_package("lodash");
    assert!(effective.access.allows(&Identity::Anonymous));
    assert!(!effective.publish.allows(&Identity::Anonymous));
    assert!(effective.publish.allows(&user("alice")));
}

#[test]
fn policy_anonymous_token_is_wired() {
    let yaml = "\
storage: ./s
uplinks: {}
packages:
  '@anon/*':
    access: $anonymous
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let anon = config.policies.for_package("@anon/x");
    assert!(anon.access.allows(&Identity::Anonymous));
    assert!(!anon.access.allows(&user("alice")));
}

#[test]
fn policy_usernames_grant_per_user_access() {
    // Bare names are usernames/groups (verdaccio-style), no longer
    // a config error.
    let yaml = "\
storage: ./s
uplinks: {}
packages:
  '@team/*':
    access: alice bob
    publish: alice
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let team = config.policies.for_package("@team/x");
    assert!(team.access.allows(&user("alice")));
    assert!(team.access.allows(&user("bob")));
    assert!(!team.access.allows(&user("carol")));
    assert!(!team.access.allows(&Identity::Anonymous));
    assert!(team.publish.allows(&user("alice")));
    assert!(!team.publish.allows(&user("bob")));
}

#[test]
fn policy_access_list_accepts_string_and_sequence_forms() {
    // verdaccio accepts both a space-separated string and a YAML
    // sequence; they must compile to the same token list.
    let as_string = "\
storage: ./s
uplinks: {}
packages:
  '@team/*':
    access: alice bob
";
    let as_sequence = "\
storage: ./s
uplinks: {}
packages:
  '@team/*':
    access: [alice, bob]
";
    for yaml in [as_string, as_sequence] {
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        let access = config.policies.for_package("@team/x").access;
        assert!(access.allows(&user("alice")), "{yaml}");
        assert!(access.allows(&user("bob")), "{yaml}");
        assert!(!access.allows(&user("carol")), "{yaml}");
    }
}

#[test]
fn bundled_default_config_enforces_its_protections() {
    // Building from the bundled YAML must reproduce the
    // registry-mock protections that used to be hard-coded.
    let config = Config::from_default_yaml(Path::new("/tmp"), listen(), None);
    let needs_auth = config.policies.for_package("@pnpm.e2e/needs-auth");
    assert!(!needs_auth.access.allows(&Identity::Anonymous));
    assert!(needs_auth.access.allows(&user("alice")));
    assert!(!config.policies.for_package("@private/foo").access.allows(&Identity::Anonymous));
    let public = config.policies.for_package("lodash");
    assert!(public.access.allows(&Identity::Anonymous));
    assert!(!public.publish.allows(&Identity::Anonymous));
}
