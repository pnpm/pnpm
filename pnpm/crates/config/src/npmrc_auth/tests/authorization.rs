use super::{
    BasicAuth, Config, DEFAULT_REGISTRY_SCOPE, EnvVar, LoadWorkspaceYamlError, NoEnv, NpmrcAuth,
    Path, RegistryCreds, TEST_CA_PEM, assert_eq, base64_encode, default_auth_token,
    scoped_auth_token,
};

#[test]
fn ignores_non_auth_keys() {
    // `Config::new()` reads `PNPM_HOME` / `XDG_DATA_HOME` via the
    // SmartDefault expression on `Config::store_dir` —
    // `default_store_dir::<Host, _, _, _>(home::home_dir,
    // env::current_dir)` — to compute `store_dir`. Both values come
    // from the real process environment, but no other test in this
    // crate mutates them anymore — the per-branch tests in
    // `defaults::tests` and `lib::tests` drive `default_store_dir`
    // through the dependency-injection seam (pnpm/pacquet#339,
    // pnpm/pnpm#11708, pnpm/pacquet#343) with fake `Sys` providers,
    // so the two `Config::new()` snapshots compared below observe
    // the same env-derived `store_dir` even under nextest's
    // in-process parallelism without an `EnvGuard` lock.
    let ini = "
store-dir=/should/not/apply
lockfile=false
hoist=false
node-linker=hoisted
";
    let config_before = Config::new();
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(config.store_dir, config_before.store_dir);
    assert_eq!(config.lockfile, config_before.lockfile);
    assert_eq!(config.hoist, config_before.hoist);
    assert_eq!(config.node_linker, config_before.node_linker);
}

#[test]
fn parses_per_registry_auth_token() {
    let ini = "//npm.pkg.github.com/pnpm/:_authToken=ghp_xxx\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(default_auth_token(&auth, "//npm.pkg.github.com/pnpm/"), Some(Some("ghp_xxx")));
}

#[test]
fn parses_package_scope_auth_under_registry_uri() {
    let ini = "\
//npm.pkg.github.com/:_authToken=registry-token
//npm.pkg.github.com/:@orgA:_authToken=org-a-token
//npm.pkg.github.com/:@orgB:_authToken=org-b-token
//reg.com/npm/:@orgA:_authToken=org-a-path-token
//localhost:4873/:@orgC:_authToken=org-c-port-token
";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(default_auth_token(&auth, "//npm.pkg.github.com/"), Some(Some("registry-token")));
    assert_eq!(
        scoped_auth_token(&auth, "//npm.pkg.github.com/", "@orgA"),
        Some(Some("org-a-token")),
    );
    assert_eq!(
        scoped_auth_token(&auth, "//npm.pkg.github.com/", "@orgB"),
        Some(Some("org-b-token")),
    );
    assert_eq!(scoped_auth_token(&auth, "//reg.com/npm/", "@orgA"), Some(Some("org-a-path-token")));
    assert_eq!(
        scoped_auth_token(&auth, "//localhost:4873/", "@orgC"),
        Some(Some("org-c-port-token")),
    );
}

#[test]
fn parses_slash_package_scope_auth_under_registry_uri() {
    let ini = "\
//npm.pkg.github.com/@orgA:_authToken=org-a-token
//npm.pkg.github.com/@orgB/:_authToken=org-b-token
//reg.com/npm/@orgA:_authToken=org-a-path-token
";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(
        scoped_auth_token(&auth, "//npm.pkg.github.com/", "@orgA"),
        Some(Some("org-a-token")),
    );
    assert_eq!(
        scoped_auth_token(&auth, "//npm.pkg.github.com/", "@orgB"),
        Some(Some("org-b-token")),
    );
    assert_eq!(scoped_auth_token(&auth, "//reg.com/npm/", "@orgA"), Some(Some("org-a-path-token")));
}

#[test]
fn package_scope_auth_from_npmrc_wins_over_registry_auth() {
    let ini = "\
//npm.pkg.github.com/:_authToken=registry-token
//npm.pkg.github.com/:@orgA:_authToken=org-a-token
";
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config
            .auth_headers
            .for_url_with_package("https://npm.pkg.github.com/pkg", Some("@orgA/pkg"))
            .as_deref(),
        Some("Bearer org-a-token"),
    );
    assert_eq!(
        config
            .auth_headers
            .for_url_with_package("https://npm.pkg.github.com/pkg", Some("@orgB/pkg"))
            .as_deref(),
        Some("Bearer registry-token"),
    );
}

#[test]
fn parses_default_auth_token_and_keys_to_registry() {
    let ini = "_authToken=top-secret\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.default_creds.auth_token.as_deref(), Some("top-secret"));

    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config.auth_headers.for_url("https://registry.npmjs.org/foo/-/foo-1.0.0.tgz").as_deref(),
        Some("Bearer top-secret"),
    );
    assert_eq!(
        config.auth_tokens_by_uri.get("//registry.npmjs.org/").map(String::as_str),
        Some("top-secret"),
    );
}

#[test]
fn env_replace_substitutes_token() {
    struct EnvWithToken;
    impl EnvVar for EnvWithToken {
        fn var(name: &str) -> Option<String> {
            (name == "TOKEN").then(|| "abc123".to_owned())
        }
    }
    let ini = "//reg.com/:_authToken=${TOKEN}\n";
    let auth = NpmrcAuth::from_ini::<EnvWithToken>(ini, Path::new(""));
    assert_eq!(default_auth_token(&auth, "//reg.com/"), Some(Some("abc123")));
}

#[test]
fn env_replace_substitutes_quoted_token_without_quotes() {
    static_env!(EnvWithToken, &[("TOKEN", "abc123")]);

    for quoted in [r#""${TOKEN}""#, "'${TOKEN}'", r#""\u0024{TOKEN}""#] {
        let ini = format!("//reg.com/:_authToken={quoted}\n");
        let auth = NpmrcAuth::from_ini::<EnvWithToken>(&ini, Path::new(""));
        assert_eq!(default_auth_token(&auth, "//reg.com/"), Some(Some("abc123")));
    }
}

#[test]
fn project_ini_ignores_quoted_auth_env_placeholders() {
    static_env!(EnvWithSecret, &[("SECRET", "leaked")]);

    for quoted in [r#""${SECRET}""#, r#""\u0024{SECRET}""#] {
        let ini = format!("//attacker.example/:_authToken={quoted}\n");
        let auth = NpmrcAuth::from_project_ini::<EnvWithSecret>(&ini, Path::new(""));

        assert!(
            auth.creds_by_scope_by_uri.is_empty(),
            "unexpected credentials: {:?}",
            auth.creds_by_scope_by_uri,
        );
        assert!(
            auth.warnings
                .iter()
                .any(|warning| warning.contains("Ignored project-level auth setting")),
            "warnings: {:?}",
            auth.warnings,
        );
    }
}

#[test]
fn project_ini_ignores_env_placeholders_in_auth_values() {
    static_env!(
        EnvWithSecret,
        &[
            ("CERT", "leaked-cert"),
            ("KEY", "leaked-key"),
            ("SECRET", "leaked"),
            ("USER", "leaked-user"),
            ("PASSWORD", "bGVha2Vk"),
        ]
    );

    let auth = NpmrcAuth::from_project_ini::<EnvWithSecret>(
        "\
registry=https://attacker.example/
//attacker.example/:_authToken=${SECRET}
//attacker.example/:cert=${CERT}
//attacker.example/:key=${KEY}
_authToken=${SECRET}
username=${USER}
_password=${PASSWORD}
cert=${CERT}
key=${KEY}
",
        Path::new(""),
    );

    assert!(auth.creds_by_scope_by_uri.is_empty());
    assert!(auth.tls_by_uri.is_empty());
    assert_eq!(auth.default_creds.auth_token, None);
    assert_eq!(auth.default_creds.username, None);
    assert_eq!(auth.default_creds.password, None);
    assert_eq!(auth.cert, None);
    assert_eq!(auth.key, None);
    assert!(
        auth.warnings.iter().any(|warning| warning.contains("Ignored project-level auth setting")),
    );

    let mut config = Config::new();
    auth.apply_to::<EnvWithSecret>(&mut config);
    assert_eq!(config.auth_headers.for_url("https://attacker.example/pkg"), None);
    assert_eq!(config.tls_by_uri.get("//attacker.example/"), None);
}

#[test]
fn basic_auth_built_from_username_and_password() {
    let raw_password = "p@ss";
    let password_b64 = base64_encode(raw_password);
    let ini = format!("//reg.com/:username=alice\n//reg.com/:_password={password_b64}\n");
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config.auth_headers.for_url("https://reg.com/").as_deref(),
        Some(format!("Basic {}", base64_encode("alice:p@ss")).as_str()),
    );
}

#[test]
fn auth_pair_base64_keys_to_basic_header() {
    let pair = base64_encode("alice:p@ss");
    let ini = format!("//reg.com/:_auth={pair}\n");
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config.auth_headers.for_url("https://reg.com/").as_deref(),
        Some(format!("Basic {pair}").as_str()),
    );
}

/// An `_auth` spelled without its `=` padding — as a shell pipeline or a
/// hand-written `.npmrc` leaves it — reaches the registry canonically
/// encoded, not verbatim (pnpm/pnpm#14257).
#[test]
fn unpadded_auth_pair_base64_is_canonically_re_encoded() {
    let padded = base64_encode("alice:pass1");
    let unpadded = padded.trim_end_matches('=');
    assert_ne!(unpadded, padded, "the fixture must exercise the padding branch");
    let ini = format!("//reg.com/:_auth={unpadded}\n");
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config.auth_headers.for_url("https://reg.com/").as_deref(),
        Some(format!("Basic {padded}").as_str()),
    );
}

/// A value that decodes only once its trailing garbage is thrown away is
/// not a credential — pnpm's `atob` rejects it, so pacquet must too.
#[test]
fn auth_pair_base64_with_a_suffix_after_its_padding_is_rejected() {
    let ini = format!("//reg.com/:_auth={}garbage\n", base64_encode("alice:p@ss"));
    let mut config = Config::new();
    let error = NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new(""))
        .build_auth_headers(&mut config)
        .expect_err("trailing garbage after the padding must fail the load");
    assert!(
        matches!(error, LoadWorkspaceYamlError::AuthInvalidBase64 { key: "_auth" }),
        "got: {error:?}",
    );
}

/// Padding with nothing to pad is not base64 — the answer `atob` gives
/// it — so it fails as an undecodable value rather than as a credential
/// that decoded but carries no separator.
#[test]
fn auth_pair_base64_of_only_padding_is_rejected_as_invalid_base64() {
    let ini = "//reg.com/:_auth=====\n";
    let mut config = Config::new();
    let error = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""))
        .build_auth_headers(&mut config)
        .expect_err("an all-padding _auth must fail the load");
    assert!(
        matches!(error, LoadWorkspaceYamlError::AuthInvalidBase64 { key: "_auth" }),
        "got: {error:?}",
    );
}

/// An `_auth` left empty — the shape an unresolved `${VAR}` leaves —
/// names no credential, so it is skipped instead of failing the load.
#[test]
fn empty_auth_pair_base64_supplies_no_header() {
    let ini = "//reg.com/:_auth=\n";
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(config.auth_headers.for_url("https://reg.com/"), None);
}

#[test]
fn auth_pair_base64_that_does_not_decode_is_rejected() {
    let ini = "//reg.com/:_auth=not*base64\n";
    let mut config = Config::new();
    let error = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""))
        .build_auth_headers(&mut config)
        .expect_err("invalid base64 in _auth must fail the load");
    assert!(
        matches!(error, LoadWorkspaceYamlError::AuthInvalidBase64 { key: "_auth" }),
        "got: {error:?}",
    );
}

#[test]
fn auth_pair_base64_without_a_colon_is_rejected() {
    let ini = format!("//reg.com/:_auth={}\n", base64_encode("alice"));
    let mut config = Config::new();
    let error = NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new(""))
        .build_auth_headers(&mut config)
        .expect_err("a passwordless _auth must fail the load");
    assert!(matches!(error, LoadWorkspaceYamlError::AuthMissingSeparator), "got: {error:?}");
}

#[test]
fn top_level_auth_pair_keys_to_default_registry_basic_header() {
    let pair = base64_encode("bob:hunter2");
    let ini = format!("_auth={pair}\n");
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config.auth_headers.for_url("https://registry.npmjs.org/").as_deref(),
        Some(format!("Basic {pair}").as_str()),
    );
}

#[test]
fn per_registry_username_password_apply_through_build_auth_headers() {
    let raw_password = "hunter2";
    let password_b64 = base64_encode(raw_password);
    let ini = format!("//reg.example/:username=alice\n//reg.example/:_password={password_b64}\n");
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config.auth_headers.for_url("https://reg.example/foo").as_deref(),
        Some(format!("Basic {}", base64_encode("alice:hunter2")).as_str()),
    );
}

/// The credentials an `updateConfig` hook reads, keyed the way pnpm's
/// `configByUri` is: every scope of a registry, with the basic-auth pair
/// decoded and the token helper split into its command.
#[test]
fn build_auth_headers_keeps_every_credential_by_scope() {
    let ini = format!(
        "//reg.example/:_authToken=registry-wide\n//reg.example/:@acme:_auth={}\n//other.example/:tokenHelper=get-token --json\n",
        base64_encode("alice:hunter2"),
    );
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new(""))
        .build_auth_headers(&mut config)
        .expect("every credential parses");
    let reg = &config.registry_creds_by_uri["//reg.example/"];
    assert_eq!(
        reg[DEFAULT_REGISTRY_SCOPE],
        RegistryCreds { auth_token: Some("registry-wide".to_string()), ..RegistryCreds::default() },
    );
    assert_eq!(
        reg["@acme"],
        RegistryCreds {
            basic_auth: Some(BasicAuth {
                username: "alice".to_string(),
                password: "hunter2".to_string(),
            }),
            ..RegistryCreds::default()
        },
    );
    assert_eq!(
        config.registry_creds_by_uri["//other.example/"][DEFAULT_REGISTRY_SCOPE].token_helper,
        Some(vec!["get-token".to_string(), "--json".to_string()]),
    );
}

/// An unresolved `${VAR}` leaves an empty token, which pnpm 11 reports as
/// no credential at all.
#[test]
fn an_empty_credential_is_not_reported_to_hooks() {
    let ini =
        "//reg.example/:_authToken=\n//pair.example/:username=alice\n//pair.example/:_password=\n";
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""))
        .build_auth_headers(&mut config)
        .expect("empty credentials do not fail the load");
    assert!(config.registry_creds_by_uri.is_empty(), "got: {:?}", config.registry_creds_by_uri);
}

/// `false` and `null` read as "not configured" on every key except the
/// legacy `proxy` one, so the environment still applies.
#[test]
fn cascade_disabling_tokens_on_the_scheme_keys_fall_through_to_env() {
    static_env!(
        AllProxyEnvs,
        &[
            ("HTTPS_PROXY", "http://https-env.example:8080"),
            ("HTTP_PROXY", "http://http-env.example:8080"),
            ("NO_PROXY", "skip.example"),
        ]
    );
    for ini in ["https-proxy=false\nhttp-proxy=false\nno-proxy=false\n", "https-proxy=null\n"] {
        let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
        let mut config = Config::new();
        auth.apply_to::<AllProxyEnvs>(&mut config);
        assert_eq!(
            config.proxy.https_proxy.as_deref(),
            Some("http://https-env.example:8080"),
            "ini={ini:?}",
        );
    }
}

/// Only the lowercase tokens are special — pnpm's INI scalars produce
/// `false` / `null` verbatim, so any other spelling is a hostname.
#[test]
fn cascade_capitalised_disabling_tokens_are_proxy_hosts() {
    static_env!(HttpsEnv, &[("HTTPS_PROXY", "http://https-env.example:8080")]);
    for (ini, expected) in
        [("proxy=False\n", "False"), ("proxy=NULL\n", "NULL"), ("https-proxy=False\n", "False")]
    {
        let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
        let mut config = Config::new();
        auth.apply_to::<HttpsEnv>(&mut config);
        assert_eq!(config.proxy.https_proxy.as_deref(), Some(expected), "ini={ini:?}");
    }
}

#[test]
fn parses_cert_and_key_from_ini() {
    let ini = "cert=cert-pem\nkey=key-pem\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.cert.as_deref(), Some("cert-pem"));
    assert_eq!(auth.key.as_deref(), Some("key-pem"));
}

/// Rescoping pins client identity per registry rather than sending it
/// to every host.
#[test]
fn applies_strict_ssl_to_config_and_rescopes_cert_key() {
    let auth = NpmrcAuth {
        strict_ssl: Some(false),
        cert: Some("cert-pem".to_string()),
        key: Some("key-pem".to_string()),
        ..NpmrcAuth::default()
    };
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.tls.strict_ssl, Some(false));
    assert_eq!(config.tls.cert, None, "unscoped cert is rescoped, not kept top-level");
    assert_eq!(config.tls.key, None);
    let scoped = config
        .tls_by_uri
        .get("//registry.npmjs.org/")
        .expect("cert/key rescoped to the npmjs default registry");
    assert_eq!(scoped.cert.as_deref(), Some("cert-pem"));
    assert_eq!(scoped.key.as_deref(), Some("key-pem"));
}

#[test]
fn cafile_reads_and_splits_into_per_cert_pems() {
    use std::io::Write;
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let bundle = format!("{TEST_CA_PEM}\n{TEST_CA_PEM}\n");
    tmp.as_file().write_all(bundle.as_bytes()).expect("write bundle");
    let auth = NpmrcAuth {
        cafile: Some(tmp.path().to_string_lossy().into_owned()),
        ..NpmrcAuth::default()
    };
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.tls.ca.len(), 2, "expected 2 split certs, got {:?}", config.tls.ca);
    for (i, pem) in config.tls.ca.iter().enumerate() {
        assert!(pem.contains("BEGIN CERTIFICATE"), "cafile split {i} missing header: {pem:?}");
        assert!(
            pem.ends_with("-----END CERTIFICATE-----"),
            "cafile split {i} missing trailing delimiter: {pem:?}",
        );
    }
}

#[test]
fn defaults_leave_tls_config_empty() {
    let mut config = Config::new();
    NpmrcAuth::default().apply_to::<NoEnv>(&mut config);
    assert!(config.tls.ca.is_empty(), "tls.ca={:?}", config.tls.ca);
    assert!(config.tls.cert.is_none(), "tls.cert={:?}", config.tls.cert);
    assert!(config.tls.key.is_none(), "tls.key={:?}", config.tls.key);
    assert_eq!(config.tls.strict_ssl, None);
    assert!(config.tls.local_address.is_none(), "tls.local_address={:?}", config.tls.local_address);
}

#[test]
fn parses_scoped_cert_and_key() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "//reg.example.com/:cert=cert-pem\n//reg.example.com/:key=key-pem\n",
        Path::new(""),
    );
    let entry = auth.tls_by_uri.get("//reg.example.com/").expect("entry present");
    assert_eq!(entry.cert.as_deref(), Some("cert-pem"));
    assert_eq!(entry.key.as_deref(), Some("key-pem"));
}

#[test]
fn parses_scoped_certfile_and_keyfile() {
    use std::io::Write;
    let tmp_cert = tempfile::NamedTempFile::new().expect("create cert tempfile");
    let tmp_key = tempfile::NamedTempFile::new().expect("create key tempfile");
    tmp_cert.as_file().write_all(b"CERT-CONTENTS").expect("write cert");
    tmp_key.as_file().write_all(b"KEY-CONTENTS").expect("write key");
    let ini = format!(
        "//reg.example.com/:certfile={}\n//reg.example.com/:keyfile={}\n",
        tmp_cert.path().display(),
        tmp_key.path().display(),
    );
    let auth = NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new(""));
    let entry = auth.tls_by_uri.get("//reg.example.com/").expect("entry present");
    assert_eq!(entry.cert.as_deref(), Some("CERT-CONTENTS"));
    assert_eq!(entry.key.as_deref(), Some("KEY-CONTENTS"));
}

#[test]
fn applies_tls_by_uri_to_config_drops_empty() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "//keep.example.com/:ca=ca-pem\n//drop.example.com/:registry=https://drop.example/\n",
        Path::new(""),
    );
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert!(config.tls_by_uri.get("//keep.example.com/").is_some(), "non-empty entry kept");
    assert!(config.tls_by_uri.get("//drop.example.com/").is_none(), "non-TLS key ignored");
}

#[test]
fn scoped_tls_keys_dont_collide_with_top_level() {
    let auth = NpmrcAuth::from_ini::<NoEnv>("ca=top-level\n", Path::new(""));
    assert_eq!(auth.ca, vec!["top-level".to_string()]);
    assert!(auth.tls_by_uri.is_empty(), "top-level `ca=` must not pollute tls_by_uri");
}

#[test]
fn url_scoped_env_reads_npm_config_auth_token() {
    static_env_with_vars!(Env, &[("npm_config_//registry.npmjs.org/:_authToken", "npm-env-token")]);
    let auth = NpmrcAuth::from_url_scoped_env::<Env>();
    assert_eq!(default_auth_token(&auth, "//registry.npmjs.org/"), Some(Some("npm-env-token")));
}

#[test]
fn url_scoped_env_reads_pnpm_config_auth_token() {
    static_env_with_vars!(
        Env,
        &[("pnpm_config_//registry.npmjs.org/:_authToken", "pnpm-env-token")]
    );
    let auth = NpmrcAuth::from_url_scoped_env::<Env>();
    assert_eq!(default_auth_token(&auth, "//registry.npmjs.org/"), Some(Some("pnpm-env-token")));
}

#[test]
fn json_env_reads_host_keyed_default_auth_token() {
    static_env!(
        Env,
        &[(
            "pnpm_config__auth",
            r#"{"https://registry.npmjs.org":{"@":{"authToken":"json-token"}}}"#
        )]
    );
    let auth = NpmrcAuth::from_json_sources::<Env>(None).expect("valid _auth");
    assert_eq!(default_auth_token(&auth, "//registry.npmjs.org/"), Some(Some("json-token")));
}

#[test]
fn json_env_reads_scoped_auth_tokens_on_shared_host() {
    static_env!(
        Env,
        &[(
            "pnpm_config__auth",
            r#"{"https://npm.pkg.github.com":{"@org-a":{"authToken":"a-tok"},"@org-b":{"authToken":"b-tok"}}}"#
        )]
    );
    let auth = NpmrcAuth::from_json_sources::<Env>(None).expect("valid _auth");
    assert_eq!(scoped_auth_token(&auth, "//npm.pkg.github.com/", "@org-a"), Some(Some("a-tok")));
    assert_eq!(scoped_auth_token(&auth, "//npm.pkg.github.com/", "@org-b"), Some(Some("b-tok")));
}

#[test]
fn json_env_rejects_deprecated_basic_auth_field() {
    // Only `authToken` is supported; the deprecated `basicAuth` /
    // `username` + `password` forms are rejected (`deny_unknown_fields`),
    // which is a hard error.
    static_env!(
        Env,
        &[("pnpm_config__auth", r#"{"https://reg.example":{"@":{"basicAuth":"any-value"}}}"#)]
    );
    assert!(NpmrcAuth::from_json_sources::<Env>(None).is_err());
}

#[test]
fn json_env_rejects_non_string_auth_token() {
    static_env!(
        Env,
        &[("pnpm_config__auth", r#"{"https://registry.example":{"@":{"authToken":123}}}"#)]
    );
    assert!(NpmrcAuth::from_json_sources::<Env>(None).is_err());
}

#[test]
fn json_env_rejects_missing_auth_token() {
    static_env!(Env, &[("pnpm_config__auth", r#"{"https://registry.example":{"@":{}}}"#)]);
    assert!(NpmrcAuth::from_json_sources::<Env>(None).is_err());
}

#[test]
fn json_env_rejects_scope_value_that_is_not_an_auth_object() {
    static_env!(Env, &[("pnpm_config__auth", r#"{"https://registry.example":{"@":"tok"}}"#)]);
    assert!(NpmrcAuth::from_json_sources::<Env>(None).is_err());
}

#[test]
fn json_env_rejects_unsupported_auth_field() {
    static_env!(
        Env,
        &[(
            "pnpm_config__auth",
            r#"{"https://registry.example":{"@":{"tokenHelper":"/bin/echo"}}}"#
        )]
    );
    assert!(NpmrcAuth::from_json_sources::<Env>(None).is_err());
}

#[test]
fn json_env_error_does_not_leak_url_credentials() {
    // The URL key carries userinfo and a query-string token — both common
    // places to embed secrets. The rejection error must not surface them.
    static_env!(
        Env,
        &[(
            "pnpm_config__auth",
            r#"{"https://user:pw@registry.example?token=secret":{"@":{"authToken":"tok"}}}"#
        )]
    );
    let error = NpmrcAuth::from_json_sources::<Env>(None).unwrap_err().to_string();
    for leak in ["user:pw", "pw@", "token=secret", "?token"] {
        assert!(!error.contains(leak), "secret fragment {leak:?} leaked into the error: {error}");
    }
}

#[test]
fn json_global_value_configures_auth_and_env_wins_on_conflict() {
    // `_auth` from the global `config.yaml` (the `global_value` argument)
    // is parsed like the env var; the env var wins on a conflicting host.
    struct EnvJson;
    impl EnvVar for EnvJson {
        fn var(name: &str) -> Option<String> {
            (name == "pnpm_config__auth").then(|| {
                r#"{"https://registry.npmjs.org":{"@":{"authToken":"env-token"}}}"#.to_string()
            })
        }
    }
    let global = serde_json::json!({
        "https://registry.npmjs.org": { "@": { "authToken": "yaml-token" } },
        "https://other.example": { "@": { "authToken": "yaml-other" } },
    });
    let auth = NpmrcAuth::from_json_sources::<EnvJson>(Some(&global)).expect("valid _auth");
    // Env wins on the conflicting host.
    assert_eq!(default_auth_token(&auth, "//registry.npmjs.org/"), Some(Some("env-token")));
    // The global-only host is preserved.
    assert_eq!(default_auth_token(&auth, "//other.example/"), Some(Some("yaml-other")));
}

// Regression test for pnpm/pnpm#12480: from_project_ini warns and drops
// auth env vars (correct default); from_ini trusts them (used when
// PNPM_CONFIG_NPMRC_AUTH_FILE explicitly points at the project .npmrc).
#[test]
fn from_project_ini_warns_on_auth_env_placeholder() {
    static_env!(Env, &[("MY_TOKEN", "secret")]);

    let auth = NpmrcAuth::from_project_ini::<Env>(
        "//registry.npmjs.org/:_authToken=${MY_TOKEN}\n",
        Path::new(""),
    );

    assert!(
        auth.warnings.iter().any(|w| w.contains("Ignored project-level auth setting")),
        "expected auth warning but got: {:?}",
        auth.warnings,
    );
    assert_eq!(
        default_auth_token(&auth, "//registry.npmjs.org/"),
        None,
        "token must not be set when project .npmrc is untrusted",
    );
}

#[test]
fn from_ini_expands_auth_env_placeholder_without_warning() {
    static_env!(Env, &[("MY_TOKEN", "secret")]);

    let auth =
        NpmrcAuth::from_ini::<Env>("//registry.npmjs.org/:_authToken=${MY_TOKEN}\n", Path::new(""));

    assert!(
        !auth.warnings.iter().any(|w| w.contains("Ignored project-level auth setting")),
        "unexpected auth warning: {:?}",
        auth.warnings,
    );
    assert_eq!(
        default_auth_token(&auth, "//registry.npmjs.org/"),
        Some(Some("secret")),
        "token must be expanded when the file is trusted via PNPM_CONFIG_NPMRC_AUTH_FILE",
    );
}
