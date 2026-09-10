use super::{
    AUTHORIZATION, Config, Ecosystem, FeatureOverrides, Identity, IndexMap, Interval, Path,
    RegistryError, TokenEnv, UpstreamAuthFile, UpstreamAuthType, UpstreamConfig, auth_header,
    listen, resolve_upstream, upstream_config_file, user,
};

#[test]
fn upstream_bearer_token_becomes_bearer_authorization() {
    let auth = UpstreamAuthFile {
        r#type: UpstreamAuthType::Bearer,
        token: Some("abc123".to_string()),
        token_env: None,
    };
    let upstream = resolve_upstream("npmjs", upstream_config_file(Some(auth), IndexMap::new()))
        .expect("bearer token resolves");
    assert_eq!(auth_header(&upstream), Some("Bearer abc123"));
}

#[test]
fn upstream_basic_token_becomes_basic_authorization_verbatim() {
    let auth = UpstreamAuthFile {
        r#type: UpstreamAuthType::Basic,
        token: Some("dXNlcjpwYXNz".to_string()),
        token_env: None,
    };
    let upstream = resolve_upstream("priv", upstream_config_file(Some(auth), IndexMap::new()))
        .expect("basic token resolves");
    assert_eq!(auth_header(&upstream), Some("Basic dXNlcjpwYXNz"));
}

#[test]
fn upstream_token_env_true_reads_npm_token() {
    let auth = UpstreamAuthFile {
        r#type: UpstreamAuthType::Bearer,
        token: None,
        token_env: Some(TokenEnv::Flag(true)),
    };
    let upstream = resolve_upstream("npmjs", upstream_config_file(Some(auth), IndexMap::new()))
        .expect("token_env: true reads NPM_TOKEN");
    assert_eq!(auth_header(&upstream), Some("Bearer default-env-token"));
}

#[test]
fn upstream_token_env_named_reads_that_var() {
    let auth = UpstreamAuthFile {
        r#type: UpstreamAuthType::Bearer,
        token: None,
        token_env: Some(TokenEnv::Named("CUSTOM_TOKEN".to_string())),
    };
    let upstream = resolve_upstream("npmjs", upstream_config_file(Some(auth), IndexMap::new()))
        .expect("named token_env reads that var");
    assert_eq!(auth_header(&upstream), Some("Bearer custom-env-token"));
}

#[test]
fn upstream_literal_token_beats_token_env() {
    let auth = UpstreamAuthFile {
        r#type: UpstreamAuthType::Bearer,
        token: Some("literal".to_string()),
        token_env: Some(TokenEnv::Named("CUSTOM_TOKEN".to_string())),
    };
    let upstream = resolve_upstream("npmjs", upstream_config_file(Some(auth), IndexMap::new()))
        .expect("literal token wins");
    assert_eq!(auth_header(&upstream), Some("Bearer literal"));
}

#[test]
fn upstream_custom_headers_are_forwarded() {
    let headers = IndexMap::from_iter([("x-custom".to_string(), "value".to_string())]);
    let upstream = resolve_upstream("npmjs", upstream_config_file(None, headers))
        .expect("custom headers resolve");
    assert_eq!(upstream.headers.get("x-custom").unwrap().to_str().unwrap(), "value");
    assert!(auth_header(&upstream).is_none());
}

#[test]
fn upstream_custom_authorization_header_overrides_auth_block() {
    let auth = UpstreamAuthFile {
        r#type: UpstreamAuthType::Bearer,
        token: Some("from-auth".to_string()),
        token_env: None,
    };
    let headers =
        IndexMap::from_iter([("authorization".to_string(), "Basic override".to_string())]);
    let upstream = resolve_upstream("npmjs", upstream_config_file(Some(auth), headers))
        .expect("custom header overrides auth-derived one");
    assert_eq!(auth_header(&upstream), Some("Basic override"));
}

#[test]
fn upstream_auth_without_resolvable_token_is_a_config_error() {
    let auth = UpstreamAuthFile {
        r#type: UpstreamAuthType::Bearer,
        token: None,
        token_env: Some(TokenEnv::Named("UNSET_VAR".to_string())),
    };
    let err = resolve_upstream("npmjs", upstream_config_file(Some(auth), IndexMap::new()))
        .expect_err("missing token must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn upstream_auth_with_empty_literal_token_is_a_config_error() {
    let auth = UpstreamAuthFile {
        r#type: UpstreamAuthType::Bearer,
        token: Some(String::new()),
        token_env: None,
    };
    let err = resolve_upstream("npmjs", upstream_config_file(Some(auth), IndexMap::new()))
        .expect_err("an empty token must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn upstream_auth_with_empty_env_token_is_a_config_error() {
    let auth = UpstreamAuthFile {
        r#type: UpstreamAuthType::Bearer,
        token: None,
        token_env: Some(TokenEnv::Named("EMPTY_TOKEN".to_string())),
    };
    let err = resolve_upstream("npmjs", upstream_config_file(Some(auth), IndexMap::new()))
        .expect_err("an empty env token must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn upstream_token_env_false_resolves_no_token_and_is_a_config_error() {
    let auth = UpstreamAuthFile {
        r#type: UpstreamAuthType::Bearer,
        token: None,
        token_env: Some(TokenEnv::Flag(false)),
    };
    let err = resolve_upstream("npmjs", upstream_config_file(Some(auth), IndexMap::new()))
        .expect_err("token_env: false reads nothing, so an auth block must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn upstream_auth_token_with_control_char_is_a_config_error() {
    let auth = UpstreamAuthFile {
        r#type: UpstreamAuthType::Bearer,
        token: Some("bad\ntoken".to_string()),
        token_env: None,
    };
    let err = resolve_upstream("npmjs", upstream_config_file(Some(auth), IndexMap::new()))
        .expect_err("a token that is not a valid header value must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn upstream_invalid_custom_header_name_is_a_config_error() {
    let headers = IndexMap::from_iter([("bad header".to_string(), "value".to_string())]);
    let err = resolve_upstream("npmjs", upstream_config_file(None, headers))
        .expect_err("a header name with a space must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn upstream_invalid_custom_header_value_is_a_config_error() {
    let headers = IndexMap::from_iter([("x-custom".to_string(), "bad\nvalue".to_string())]);
    let err = resolve_upstream("npmjs", upstream_config_file(None, headers))
        .expect_err("a header value with a control char must error");
    assert!(matches!(err, RegistryError::InvalidConfig { .. }));
}

#[test]
fn from_yaml_str_resolves_upstream_auth_and_headers() {
    let yaml = r"
registries:
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    access: $authenticated
    auth:
      type: bearer
      token: secret-token
    headers:
      X-Org: acme
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let upstream = &config.upstreams["npmjs"];
    assert_eq!(
        upstream.headers.get(AUTHORIZATION).unwrap().to_str().unwrap(),
        "Bearer secret-token",
    );
    assert_eq!(upstream.headers.get("x-org").unwrap().to_str().unwrap(), "acme");
}

#[test]
fn parses_cors_origins_and_upstream_search() {
    let yaml = r"
cors:
  allowedOrigins:
    - https://npmx.dev
    - http://localhost:3000/
registries:
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
    search: true
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    assert_eq!(
        config.cors.allowed_origins(),
        ["https://npmx.dev".to_string(), "http://localhost:3000".to_string()],
    );
    assert!(config.upstreams["npmjs"].search);
}

/// A non-`public` upstream registry must declare who may reach it.
#[test]
fn from_yaml_str_rejects_private_upstream_without_access() {
    let yaml = "\
storage: ./s
registries:
  corp:
    type: upstream
    url: https://npm.corp.example/
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect_err("private upstream without access must be rejected");
    assert!(err.to_string().contains("public: true"), "unexpected error: {err}");
}

/// A `public` upstream is anonymous, so declaring `access:` on it is a
/// contradiction that must fail closed rather than be silently dropped.
#[test]
fn from_yaml_str_rejects_public_upstream_with_access() {
    let yaml = "\
storage: ./s
registries:
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
    access: $authenticated
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None)
        .expect_err("public upstream with access must be rejected");
    assert!(err.to_string().contains("`access`"), "unexpected error: {err}");
}

/// A `public` upstream is fetched anonymously, so *any* custom header — not just
/// `Authorization`, but a credential smuggled through `X-Api-Key` — must fail
/// closed rather than be sent to a supposedly-public origin.
#[test]
fn from_yaml_str_rejects_public_upstream_with_custom_headers() {
    for header in ["Authorization: Bearer leaked", "X-Api-Key: secret"] {
        let yaml = format!(
            "\
storage: ./s
registries:
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
    headers:
      {header}
",
        );
        let err = Config::from_yaml_str(&yaml, Path::new("/x"), listen(), None)
            .expect_err("public upstream with a custom header must be rejected");
        assert!(err.to_string().contains("headers"), "unexpected error for {header:?}: {err}");
    }
}

/// With the registry disabled, an upstream registry whose credential cannot
/// resolve must not fail startup — the tier never talks to that upstream. The
/// registry still joins the (validated) graph; only its serving config is
/// skipped.
#[test]
fn cli_disable_registry_skips_upstream_registry_credentials_but_keeps_the_graph() {
    let yaml = "\
storage: ./s
registries:
  corp:
    type: upstream
    url: https://corp.example/npm/
    access: [team]
    auth:
      type: bearer
      token_env: PNPR_DEFINITELY_UNSET_TOKEN_VAR
  main:
    type: router
    sources: [corp]
";
    let overrides = FeatureOverrides {
        disable_registry: true,
        disable_resolver: false,
        disable_artifacts: false,
    };
    let config =
        Config::from_yaml_str_with_overrides(yaml, Path::new("/x"), listen(), None, overrides)
            .expect("a resolver-only tier must not fail on unused upstream credentials");
    assert!(config.upstreams.is_empty(), "credentials must not be resolved or carried");
    assert!(config.registries.get("main").is_some(), "the graph is still built and validated");
}

#[test]
fn teams_grant_package_and_upstream_access() {
    let yaml = r"
registries:
  local:
    type: hosted
    teams:
      platform: [alice, bob]
    packages:
      '@team/*':
        access: team:platform
  corp:
    type: upstream
    url: https://npm.corp.example/
    teams:
      partners: [alice]
    access: team:partners
    auth:
      type: bearer
      token: corp-token
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let alice = Identity::user("alice");
    let bob = Identity::user("bob");
    let carol = Identity::user("carol");

    let rules = &config.hosted["local"].rules;
    let team = rules.for_package("@team/widget");
    assert!(team.access.allows(&alice));
    assert!(team.access.allows(&bob));
    assert!(!team.access.allows(&carol));
    assert!(!team.access.allows(&Identity::Anonymous));

    let access = config.upstreams["corp"].access.as_ref().expect("upstream declares access");
    assert!(access.allows(&alice));
    assert!(!access.allows(&bob));
}

#[test]
fn upstream_write_rules_are_rejected() {
    // No write can land on an upstream, so a `publish`/`unpublish` value in
    // its `packages:` map is a config mistake — and it fails on every tier,
    // whether or not the registry surface resolves upstream credentials.
    let yaml = "\
storage: ./s
registries:
  corp:
    type: upstream
    url: https://npm.corp.example/
    public: true
    packages:
      '@corp/*':
        publish: $authenticated
";
    let err = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap_err();
    assert!(
        matches!(
            &err,
            RegistryError::InvalidConfig { reason }
                if reason.contains("publish") && reason.contains("upstream"),
        ),
        "unexpected error: {err}",
    );
}

#[test]
fn public_upstream_allows_per_package_access_rules() {
    // `public: true` describes the upstream *fetch* (anonymous, no
    // credential, no registry-level access default). A per-package `access`
    // rule still gates who may read the name through pnpr.
    let yaml = "\
storage: ./s
registries:
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
    packages:
      '@internal/*':
        access: $authenticated
      '**': {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let rules = &config.upstreams["npmjs"].rules;
    assert!(!rules.for_package("@internal/x").access.allows(&Identity::Anonymous));
    assert!(rules.for_package("@internal/x").access.allows(&user("alice")));
    assert!(rules.for_package("lodash").access.allows(&Identity::Anonymous));
}

#[test]
fn upstream_packages_map_bounds_the_namespace() {
    let yaml = "\
storage: ./s
registries:
  corp:
    type: upstream
    url: https://npm.corp.example/
    access: $authenticated
    packages:
      '@corp/*': {}
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    use pnpr_registry::{ConcreteKind, Resolved};
    assert_eq!(
        config.registries.resolve("corp", Ecosystem::Npm, "@corp/tool"),
        Resolved::Concrete { registry: "corp", kind: ConcreteKind::Upstream },
    );
    assert_eq!(config.registries.resolve("corp", Ecosystem::Npm, "lodash"), Resolved::Unclaimed);
}

#[test]
fn resolve_upstream_config_defaults_knobs_to_verdaccio_values() {
    let upstream = resolve_upstream("npmjs", upstream_config_file(None, IndexMap::new())).unwrap();
    // An unset `maxage` defers to the global packument TTL (`None` here),
    // while the rest fall back to verdaccio's documented defaults.
    assert_eq!(upstream.maxage, None);
    assert_eq!(upstream.timeout, UpstreamConfig::DEFAULT_TIMEOUT);
    assert_eq!(upstream.max_fails, UpstreamConfig::DEFAULT_MAX_FAILS);
    assert_eq!(upstream.fail_timeout, UpstreamConfig::DEFAULT_FAIL_TIMEOUT);
    assert!(upstream.cache);
    assert!(!upstream.search);
}

#[test]
fn resolve_upstream_config_parses_explicit_knobs() {
    use std::time::Duration;
    let mut file = upstream_config_file(None, IndexMap::new());
    file.maxage = Some(Interval("10m".to_string()));
    file.timeout = Some(Interval("45s".to_string()));
    file.max_fails = Some(5);
    file.fail_timeout = Some(Interval("1m".to_string()));
    file.cache = Some(false);
    file.search = true;
    let upstream = resolve_upstream("npmjs", file).unwrap();
    assert_eq!(upstream.maxage, Some(Duration::from_mins(10)));
    assert_eq!(upstream.timeout, Duration::from_secs(45));
    assert_eq!(upstream.max_fails, 5);
    assert_eq!(upstream.fail_timeout, Duration::from_mins(1));
    assert!(!upstream.cache);
    assert!(upstream.search);
}

#[test]
fn resolve_upstream_config_rejects_an_unparsable_interval() {
    let mut file = upstream_config_file(None, IndexMap::new());
    file.maxage = Some(Interval("whenever".to_string()));
    let err = resolve_upstream("npmjs", file).unwrap_err();
    assert!(
        matches!(err, RegistryError::InvalidConfig { reason } if reason.contains("maxage")),
        "expected an InvalidConfig naming the offending field",
    );
}

#[test]
fn upstream_resolves_bearer_auth_and_access() {
    let yaml = r"
registries:
  corp:
    type: upstream
    url: https://npm.corp.example/
    access: [$authenticated, alice]
    auth:
      type: bearer
      token: corp-token
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let upstream = &config.upstreams["corp"];
    assert_eq!(upstream.url, "https://npm.corp.example/");
    assert_eq!(auth_header(upstream), Some("Bearer corp-token"));
    let access = upstream.access.as_ref().expect("upstream declares access");
    assert!(access.allows(&user("alice")));
    assert!(!access.allows(&Identity::Anonymous));
}

#[test]
fn upstream_resolves_basic_auth_and_access() {
    let yaml = r"
registries:
  corp:
    type: upstream
    url: https://npm.corp.example/
    access: $authenticated
    auth:
      type: basic
      token: dXNlcjpwYXNz
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    let upstream = &config.upstreams["corp"];
    assert_eq!(auth_header(upstream), Some("Basic dXNlcjpwYXNz"));
    let access = upstream.access.as_ref().expect("upstream declares access");
    assert!(access.allows(&user("bob")));
    assert!(!access.allows(&Identity::Anonymous));
}

#[test]
fn public_upstream_registry_carries_no_access_credential() {
    let yaml = r"
registries:
  corp:
    type: upstream
    url: https://npm.corp.example/
    public: true
";
    let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
    // A public upstream registry is reachable anonymously and carries no access
    // policy or upstream credential.
    assert!(config.upstreams["corp"].access.is_none());
}
