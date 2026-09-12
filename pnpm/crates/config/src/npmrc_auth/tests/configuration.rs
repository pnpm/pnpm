use super::{
    Config, EnvVar, NoEnv, NoProxySetting, NpmrcAuth, Path, TEST_CA_PEM, assert_eq,
    default_auth_token,
};

/// pnpm keeps only auth/registry keys when reading an `.npmrc`
/// (`isNpmrcReadableKey`), and `scope` is not among them, so a `scope=` line
/// there is dropped rather than becoming the default login scope.
#[test]
fn scope_is_ignored_in_npmrc() {
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>("scope=@my-org\n", Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(config.scope, None);
}

#[test]
fn project_ini_ignores_env_placeholders_in_registry_urls() {
    static_env!(EnvWithSecret, &[("SECRET", "leaked")]);

    let auth = NpmrcAuth::from_project_ini::<EnvWithSecret>(
        "registry=https://registry.example.com/${SECRET}/\n",
        Path::new(""),
    );

    assert_eq!(auth.registry, None);
    assert!(auth.warnings.iter().any(|warning| warning.contains("registry")));

    let mut config = Config::new();
    auth.apply_to::<EnvWithSecret>(&mut config);
    assert!(!config.registry.contains("leaked"));
}

#[test]
fn project_ini_ignores_env_placeholders_in_scoped_registry_urls() {
    static_env!(EnvWithSecret, &[("SECRET", "leaked")]);

    let auth = NpmrcAuth::from_project_ini::<EnvWithSecret>(
        "@scope:registry=https://registry.example.com/${SECRET}/\n",
        Path::new(""),
    );

    assert!(auth.scoped_registries.is_empty());
    assert!(auth.warnings.iter().any(|warning| warning.contains("@scope:registry")));
}

#[test]
fn trusted_ini_expands_env_placeholders_in_registry_urls() {
    static_env!(EnvWithSecret, &[("SECRET", "trusted")]);

    let auth = NpmrcAuth::from_ini::<EnvWithSecret>(
        "registry=https://registry.example.com/${SECRET}/\n",
        Path::new(""),
    );

    assert_eq!(auth.registry.as_deref(), Some("https://registry.example.com/trusted/"));
}

#[test]
fn project_ini_ignores_env_placeholders_in_url_scoped_keys() {
    static_env!(EnvWithSecret, &[("SECRET", "leaked")]);

    let auth = NpmrcAuth::from_project_ini::<EnvWithSecret>(
        "//registry.example.com/${SECRET}/:_authToken=token\n",
        Path::new(""),
    );

    assert!(auth.creds_by_scope_by_uri.is_empty());
    assert!(
        auth.warnings
            .iter()
            .any(|warning| warning.contains("//registry.example.com/${SECRET}/:_authToken")),
    );
}

#[test]
fn project_ini_ignores_env_placeholders_in_proxy_urls() {
    static_env!(EnvWithSecret, &[("SECRET", "leaked")]);

    let auth = NpmrcAuth::from_project_ini::<EnvWithSecret>(
        "\
https-proxy=http://proxy.example.com/${SECRET}/
http-proxy=http://proxy.example.com/${SECRET}/
proxy=http://legacy-proxy.example.com/${SECRET}/
",
        Path::new(""),
    );

    assert_eq!(auth.https_proxy, None);
    assert_eq!(auth.http_proxy, None);
    assert_eq!(auth.legacy_proxy, None);
    assert!(
        auth.warnings
            .iter()
            .any(|warning| warning.contains("Ignored project-level request destination")),
    );
}

#[test]
fn env_replace_failure_warns_and_drops_unresolved_to_empty() {
    let ini = "//reg.com/:_authToken=${MISSING}\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(default_auth_token(&auth, "//reg.com/"), Some(Some("")));
    assert_eq!(auth.warnings.len(), 1);
    assert!(auth.warnings[0].contains("${MISSING}"));
}

#[test]
fn env_replace_failure_preserves_resolved_and_default_placeholders() {
    struct EnvWithSet;
    impl EnvVar for EnvWithSet {
        fn var(name: &str) -> Option<String> {
            (name == "SET").then(|| "AAA".to_owned())
        }
    }
    let ini = "//reg.com/:_authToken=${SET}-${UNSET}-${DEFAULTED:-fallback}\n";
    let auth = NpmrcAuth::from_ini::<EnvWithSet>(ini, Path::new(""));
    assert_eq!(default_auth_token(&auth, "//reg.com/"), Some(Some("AAA--fallback")));
    assert_eq!(auth.warnings.len(), 1);
    assert!(auth.warnings[0].contains("${UNSET}"));
}

#[test]
fn env_replace_failure_on_key_warns_and_drops_unresolved_to_empty() {
    let ini = "${MISSING}_authToken=abc\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.default_creds.auth_token.as_deref(), Some("abc"));
    assert!(auth.warnings.iter().any(|warning| warning.contains("${MISSING}")));
}

#[test]
fn cascade_env_fallback_only_fires_when_npmrc_unset() {
    static_env!(
        AllProxyEnvs,
        &[
            ("HTTPS_PROXY", "http://https-env.example:8080"),
            ("HTTP_PROXY", "http://http-env.example:8080"),
            ("NO_PROXY", "skip.example"),
        ]
    );
    let auth = NpmrcAuth::default();
    let mut config = Config::new();
    auth.apply_to::<AllProxyEnvs>(&mut config);
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://https-env.example:8080"));
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://https-env.example:8080"));
    assert_eq!(config.proxy.no_proxy, Some(NoProxySetting::List(vec!["skip.example".to_string()])));
}

#[test]
fn cascade_npmrc_value_wins_over_env() {
    static_env!(
        ConflictingEnv,
        &[("HTTPS_PROXY", "http://env.example:8080"), ("NO_PROXY", "env.example")]
    );
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "https-proxy=http://npmrc.example:8080\nno-proxy=npmrc.example\n",
        Path::new(""),
    );
    let mut config = Config::new();
    auth.apply_to::<ConflictingEnv>(&mut config);
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://npmrc.example:8080"));
    assert_eq!(
        config.proxy.no_proxy,
        Some(NoProxySetting::List(vec!["npmrc.example".to_string()])),
    );
}

#[test]
fn cascade_http_proxy_env_fallback_chain_proxy_var() {
    static_env!(BareProxy, &[("PROXY", "http://barenv.example:80")]);
    let auth = NpmrcAuth::default();
    let mut config = Config::new();
    auth.apply_to::<BareProxy>(&mut config);
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://barenv.example:80"));
    assert_eq!(config.proxy.https_proxy, None);
}

#[test]
fn cascade_empty_npmrc_proxy_keys_fall_through_to_env() {
    static_env!(
        AllProxyEnvs,
        &[
            ("HTTPS_PROXY", "http://https-env.example:8080"),
            ("HTTP_PROXY", "http://http-env.example:8080"),
            ("NO_PROXY", "skip.example"),
        ]
    );
    let auth =
        NpmrcAuth::from_ini::<NoEnv>("https-proxy=\nhttp-proxy=\nno-proxy=\n", Path::new(""));
    let mut config = Config::new();
    auth.apply_to::<AllProxyEnvs>(&mut config);
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://https-env.example:8080"));
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://https-env.example:8080"));
    assert_eq!(config.proxy.no_proxy, Some(NoProxySetting::List(vec!["skip.example".to_string()])));
}

/// An empty env var still shadows the lower-priority env vars below it,
/// leaving no proxy — the TypeScript CLI resolves `HTTPS_PROXY=` the same
/// way, and the empty value is dropped when the client is built.
#[test]
fn cascade_empty_https_proxy_env_shadows_http_proxy_env() {
    static_env!(
        EmptyHttpsEnv,
        &[("HTTPS_PROXY", ""), ("HTTP_PROXY", "http://http-env.example:8080")]
    );
    let auth = NpmrcAuth::default();
    let mut config = Config::new();
    auth.apply_to::<EmptyHttpsEnv>(&mut config);
    assert_eq!(config.proxy.https_proxy.as_deref(), Some(""));
    assert_eq!(config.proxy.http_proxy.as_deref(), Some(""));
}

#[test]
fn cascade_env_var_lowercase_lookup() {
    static_env!(LowercaseEnv, &[("https_proxy", "http://lower.example:8080")]);
    let auth = NpmrcAuth::default();
    let mut config = Config::new();
    auth.apply_to::<LowercaseEnv>(&mut config);
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://lower.example:8080"));
}

// Regression for <https://github.com/pnpm/pnpm/issues/11624>.
#[test]
fn cafile_relative_path_resolves_against_npmrc_dir() {
    let npmrc_dir = tempfile::tempdir().expect("tempdir");
    let auth = NpmrcAuth::from_ini::<NoEnv>("cafile=certs/ca.pem\n", npmrc_dir.path());
    let expected = npmrc_dir.path().join("certs/ca.pem").to_string_lossy().into_owned();
    assert_eq!(auth.cafile.as_deref(), Some(expected.as_str()));
}

#[test]
fn applies_inline_ca_to_config() {
    let auth = NpmrcAuth { ca: vec![TEST_CA_PEM.to_string()], ..NpmrcAuth::default() };
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.tls.ca.len(), 1);
    let first = &config.tls.ca[0];
    assert!(first.contains("BEGIN CERTIFICATE"), "inline CA missing header: {first:?}");
}

#[test]
fn url_scoped_env_pnpm_prefix_wins_over_npm() {
    static_env_with_vars!(
        Env,
        &[
            ("npm_config_//registry.npmjs.org/:_authToken", "npm-env-token"),
            ("pnpm_config_//registry.npmjs.org/:_authToken", "pnpm-env-token"),
        ]
    );
    let auth = NpmrcAuth::from_url_scoped_env::<Env>();
    assert_eq!(default_auth_token(&auth, "//registry.npmjs.org/"), Some(Some("pnpm-env-token")));
}

#[test]
fn url_scoped_env_ignores_non_url_and_empty_values() {
    static_env_with_vars!(
        Env,
        &[
            ("npm_config_registry", "https://example.test/"),
            ("npm_config_//empty.example/:_authToken", ""),
            ("PATH", "/usr/bin"),
        ]
    );
    let auth = NpmrcAuth::from_url_scoped_env::<Env>();
    assert!(auth.creds_by_scope_by_uri.is_empty());
    assert!(auth.registry.is_none());
}

#[test]
fn url_scoped_env_ignores_non_ascii_names_without_panicking() {
    // A multi-byte env var name must not panic the byte-index prefix check
    // in `parse_url_scoped_env_name` (regression: `name[..prefix.len()]`).
    static_env_with_vars!(
        Env,
        &[
            ("プログラム_config_//registry.example/:_authToken", "ignored"),
            ("ñpm_config_//registry.example/:_authToken", "ignored"),
        ]
    );
    let auth = NpmrcAuth::from_url_scoped_env::<Env>();
    assert!(auth.creds_by_scope_by_uri.is_empty());
}

#[test]
fn json_env_accepts_uppercase_scheme() {
    // URL schemes are case-insensitive; the TS side parses keys with
    // `new URL()`, which accepts `HTTPS://...`. pacquet must too.
    static_env!(
        Env,
        &[(
            "pnpm_config__auth",
            r#"{"HTTPS://registry.npmjs.org":{"@":{"authToken":"upper-scheme-token"}}}"#
        )]
    );
    let auth = NpmrcAuth::from_json_sources::<Env>(None).expect("valid _auth");
    assert_eq!(
        default_auth_token(&auth, "//registry.npmjs.org/"),
        Some(Some("upper-scheme-token")),
    );
}

#[test]
fn json_env_duplicate_route_keeps_last_in_source_order() {
    // Two hosts both route the default registry; the source-LAST entry wins,
    // matching pnpm's `Object.entries` iteration. Listed reverse-alphabetically
    // so a sorted map would pick the wrong host.
    static_env!(
        Env,
        &[(
            "pnpm_config__auth",
            r#"{"https://zzz.example":{"@":{"authToken":"z"}},"https://aaa.example":{"@":{"authToken":"a"}}}"#
        )]
    );
    let auth = NpmrcAuth::from_json_sources::<Env>(None).expect("valid _auth");
    assert_eq!(
        auth.json_env_registries.get("default").map(String::as_str),
        Some("https://aaa.example/"),
    );
}

#[test]
fn json_env_default_scope_infers_default_registry_route() {
    static_env!(
        Env,
        &[("pnpm_config__auth", r#"{"https://my-npm-proxy.example":{"@":{"authToken":"tok"}}}"#)]
    );
    let auth = NpmrcAuth::from_json_sources::<Env>(None).expect("valid _auth");
    assert_eq!(
        auth.json_env_registries.get("default").map(String::as_str),
        Some("https://my-npm-proxy.example/"),
    );
}

#[test]
fn json_env_package_scope_infers_scoped_registry_route() {
    static_env!(
        Env,
        &[("pnpm_config__auth", r#"{"https://npm.pkg.github.com":{"@org":{"authToken":"tok"}}}"#)]
    );
    let auth = NpmrcAuth::from_json_sources::<Env>(None).expect("valid _auth");
    assert_eq!(
        auth.json_env_registries.get("@org").map(String::as_str),
        Some("https://npm.pkg.github.com/"),
    );
    assert!(!auth.json_env_registries.contains_key("default"));
}

#[test]
fn json_env_honors_upper_case_form() {
    // Both `pnpm_config__auth` (documented) and `PNPM_CONFIG__AUTH` (the
    // all-caps shell convention) are accepted — mirrors `read_pnpm_env`'s
    // two-form lookup shape.
    static_env!(
        Env,
        &[(
            "PNPM_CONFIG__AUTH",
            r#"{"https://registry.npmjs.org":{"@":{"authToken":"upper-token"}}}"#
        )]
    );
    let auth = NpmrcAuth::from_json_sources::<Env>(None).expect("valid _auth");
    assert_eq!(default_auth_token(&auth, "//registry.npmjs.org/"), Some(Some("upper-token")));
}

#[test]
fn json_env_lower_case_wins_over_upper_case_when_both_are_set() {
    // Both forms are accepted; when both are set, the documented
    // (lowercase) form wins. Mirrors `read_pnpm_env`'s two-form lookup.
    static_env!(
        Env,
        &[
            (
                "pnpm_config__auth",
                r#"{"https://registry.npmjs.org":{"@":{"authToken":"lower-token"}}}"#
            ),
            (
                "PNPM_CONFIG__AUTH",
                r#"{"https://registry.npmjs.org":{"@":{"authToken":"upper-token"}}}"#
            ),
        ]
    );
    let auth = NpmrcAuth::from_json_sources::<Env>(None).expect("valid _auth");
    assert_eq!(default_auth_token(&auth, "//registry.npmjs.org/"), Some(Some("lower-token")));
}

#[test]
fn json_env_empty_lowercase_falls_back_to_uppercase() {
    static_env!(
        Env,
        &[
            ("pnpm_config__auth", ""),
            (
                "PNPM_CONFIG__AUTH",
                r#"{"https://registry.npmjs.org":{"@":{"authToken":"upper-token"}}}"#
            ),
        ]
    );
    let auth = NpmrcAuth::from_json_sources::<Env>(None).expect("valid _auth");
    assert_eq!(default_auth_token(&auth, "//registry.npmjs.org/"), Some(Some("upper-token")));
}

#[test]
fn json_env_rejects_host_value_that_is_not_a_scope_object() {
    static_env!(Env, &[("pnpm_config__auth", r#"{"https://registry.example":123}"#)]);
    assert!(NpmrcAuth::from_json_sources::<Env>(None).is_err());
}

#[test]
fn json_env_rejects_host_key_that_is_not_a_registry_url() {
    static_env!(Env, &[("pnpm_config__auth", r#"{"not a url":{"@":{"authToken":"tok"}}}"#)]);
    let error = NpmrcAuth::from_json_sources::<Env>(None).unwrap_err().to_string();
    assert!(!error.contains("not a url"), "raw key must not leak into the error: {error}");
}

#[test]
fn json_env_rejects_non_http_scheme() {
    static_env!(
        Env,
        &[("pnpm_config__auth", r#"{"ftp://registry.example":{"@":{"authToken":"tok"}}}"#)]
    );
    assert!(NpmrcAuth::from_json_sources::<Env>(None).is_err());
}

#[test]
fn json_env_rejects_invalid_scope_name() {
    static_env!(
        Env,
        &[("pnpm_config__auth", r#"{"https://registry.example":{"org":{"authToken":"tok"}}}"#)]
    );
    assert!(NpmrcAuth::from_json_sources::<Env>(None).is_err());
}

#[test]
fn json_env_rejects_malformed_json() {
    static_env!(Env, &[("pnpm_config__auth", "{ not valid json")]);
    assert!(NpmrcAuth::from_json_sources::<Env>(None).is_err());
}

#[test]
fn json_env_rejects_non_object_top_level() {
    // Arrays expose their indices as keys, so reject them outright.
    static_env!(Env, &[("pnpm_config__auth", r#"["//registry.example/:_authToken","sneaky"]"#)]);
    assert!(NpmrcAuth::from_json_sources::<Env>(None).is_err());
}

#[test]
fn json_env_empty_or_unset_returns_default() {
    // Unset: `Sys::var` returns `None` for both forms.
    struct UnsetEnv;
    impl EnvVar for UnsetEnv {
        fn var(_: &str) -> Option<String> {
            None
        }
    }
    let auth = NpmrcAuth::from_json_sources::<UnsetEnv>(None).expect("valid _auth");
    assert!(auth.creds_by_scope_by_uri.is_empty());

    // Set but empty: filtered out by the `.filter(|v| !v.is_empty())` step.
    static_env!(EnvWithEmptyAuth, &[("pnpm_config__auth", "")]);
    let auth = NpmrcAuth::from_json_sources::<EnvWithEmptyAuth>(None).expect("valid _auth");
    assert!(auth.creds_by_scope_by_uri.is_empty());
}

#[test]
fn json_env_normalizes_registry_url_key() {
    // The `url` crate lowercases scheme + host and drops the default port,
    // matching the TS side's `new URL()`.
    static_env!(
        Env,
        &[("pnpm_config__auth", r#"{"HTTPS://Reg.Example:443":{"@":{"authToken":"tok"}}}"#)]
    );
    let auth = NpmrcAuth::from_json_sources::<Env>(None).expect("valid _auth");
    assert_eq!(default_auth_token(&auth, "//reg.example/"), Some(Some("tok")));
    assert_eq!(
        auth.json_env_registries.get("default").map(String::as_str),
        Some("https://reg.example/"),
    );
}
