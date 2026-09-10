use super::{
    Config, EnvVar, NoEnv, NoProxySetting, NpmrcAuth, Path, RawCreds, TEST_CA_PEM, assert_eq,
    base64_decode, base64_encode, default_auth_token,
};

#[test]
fn picks_up_registry_and_normalises_trailing_slash() {
    let ini = "registry=https://r.example\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.registry.as_deref(), Some("https://r.example"));

    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.registry, "https://r.example/");
}

#[test]
fn preserves_existing_trailing_slash() {
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>("registry=https://r.example/\n", Path::new(""))
        .apply_to::<NoEnv>(&mut config);
    assert_eq!(config.registry, "https://r.example/");
}

#[test]
fn parses_scoped_registry_and_applies() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "@private:registry=https://private.example/npm\n",
        Path::new(""),
    );

    assert_eq!(
        auth.scoped_registries.get("@private").map(String::as_str),
        Some("https://private.example/npm/"),
    );

    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config.registries_by_scope.get("@private").map(String::as_str),
        Some("https://private.example/npm/"),
    );
}

#[test]
fn parses_ini_quoted_values() {
    for (quoted, expected) in [
        (r#""literal-token""#, "literal-token"),
        ("'literal-token'", "literal-token"),
        (r#""token\nline""#, "token\nline"),
        ("'", ""),
        (r#""unterminated"#, r#""unterminated"#),
        ("'unterminated", "'unterminated"),
    ] {
        let ini = format!("//reg.com/:_authToken={quoted}\n");
        let auth = NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new(""));
        assert_eq!(default_auth_token(&auth, "//reg.com/"), Some(Some(expected)));
    }
}

#[test]
fn project_ini_keeps_literal_dollar_brace_fragments() {
    let auth = NpmrcAuth::from_project_ini::<NoEnv>(
        "//attacker.example/:_authToken=literal${token\n",
        Path::new(""),
    );

    assert_eq!(default_auth_token(&auth, "//attacker.example/"), Some(Some("literal${token")));
    assert_eq!(auth.warnings, Vec::<String>::new());
}

/// `[section]`-style headers are not legal `.npmrc` syntax (npm's
/// rc files are flat key/value pairs).
#[test]
fn ini_section_headers_are_dropped_silently() {
    let ini = "[default]\nregistry=https://r.example\n[other]\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.registry.as_deref(), Some("https://r.example"));
    assert_eq!(auth.warnings, Vec::<String>::new());
}

#[test]
fn top_level_username_password_keys_to_default_registry_basic_header() {
    let raw_password = "hunter2";
    let password_b64 = base64_encode(raw_password);
    let ini = format!("username=bob\n_password={password_b64}\n");
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config.auth_headers.for_url("https://registry.npmjs.org/").as_deref(),
        Some(format!("Basic {}", base64_encode("bob:hunter2")).as_str()),
    );
}

#[test]
fn lone_per_registry_password_produces_no_header() {
    let ini = format!("//reg.com/:_password={}\n", base64_encode("solo"));
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(config.auth_headers.for_url("https://reg.com/"), None);
}

#[test]
fn unknown_per_registry_suffix_is_silently_dropped() {
    let ini = "//reg.example/:registry=https://other.example/\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert!(auth.creds_by_scope_by_uri.is_empty());
    assert_eq!(auth.default_creds, RawCreds::default());
    assert_eq!(auth.warnings, Vec::<String>::new());
}

/// Pnpm's `parseBasicAuth` doesn't have this exact fallback (it always
/// `atob`s), but pacquet's tolerance avoids losing the credential
/// for `.npmrc` files where `_password` was already a raw value.
#[test]
fn invalid_base64_password_falls_back_to_raw_value() {
    let ini = "//reg.com/:username=alice\n//reg.com/:_password=raw*pw\n";
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config.auth_headers.for_url("https://reg.com/").as_deref(),
        Some(format!("Basic {}", base64_encode("alice:raw*pw")).as_str()),
    );
}

/// Without these assertions the password-decode fallback
/// (`unwrap_or_else(... pass_b64.clone())`) path stays unreachable
/// from the parser tests.
///
/// Every case here is an `atob` result: the decoder answers what pnpm's
/// `decodeBase64Credential` answers for the same value.
#[test]
fn base64_decode_matches_atob() {
    assert_eq!(base64_decode(&base64_encode("alice:hunter2")).as_deref(), Some("alice:hunter2"));
    assert_eq!(base64_decode("Pz8/").as_deref(), Some("???"));
    assert_eq!(base64_decode("fn5+").as_deref(), Some("~~~"));
    assert_eq!(base64_decode("aGk=").as_deref(), Some("hi"));
    // Padding may be redundant, short of what the value needs, or
    // missing entirely, and whitespace may appear anywhere.
    assert_eq!(base64_decode("aGk===").as_deref(), Some("hi"));
    assert_eq!(base64_decode("Zm9vOmJhcg=").as_deref(), Some("foo:bar"));
    assert_eq!(base64_decode("aGk").as_deref(), Some("hi"));
    assert_eq!(base64_decode("aG k=").as_deref(), Some("hi"));
    // A truncated final group keeps the whole bytes it does carry.
    assert_eq!(base64_decode("aH").as_deref(), Some("h"));
    // A lone trailing character carries no whole byte of its own.
    assert_eq!(base64_decode("aGkyM"), None);
    // `=` is padding, so anything after it is not base64, and padding
    // with nothing to pad is not base64 either.
    assert_eq!(base64_decode("Zm9vOmJhcg==garbage"), None);
    assert_eq!(base64_decode("aGk=x"), None);
    assert_eq!(base64_decode("===="), None);
    assert_eq!(base64_decode("not*base64"), None);
    // Only an empty value decodes to nothing.
    assert_eq!(base64_decode("").as_deref(), Some(""));
    assert_eq!(base64_decode("  ").as_deref(), Some(""));
}

// --- Proxy parsing and cascade tests ---

#[test]
fn parses_https_proxy_from_ini() {
    let auth =
        NpmrcAuth::from_ini::<NoEnv>("https-proxy=http://proxy.example:8080\n", Path::new(""));
    assert_eq!(auth.https_proxy.as_deref(), Some("http://proxy.example:8080"));
}

#[test]
fn parses_http_proxy_from_ini() {
    let auth =
        NpmrcAuth::from_ini::<NoEnv>("http-proxy=http://proxy.example:3128\n", Path::new(""));
    assert_eq!(auth.http_proxy.as_deref(), Some("http://proxy.example:3128"));
}

#[test]
fn parses_legacy_proxy_key_from_ini() {
    let auth = NpmrcAuth::from_ini::<NoEnv>("proxy=http://legacy.example:8080\n", Path::new(""));
    assert_eq!(auth.legacy_proxy.as_deref(), Some("http://legacy.example:8080"));
    assert_eq!(auth.https_proxy, None, "legacy `proxy` is its own slot");
}

#[test]
fn no_proxy_and_noproxy_aliases_last_wins() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "no-proxy=first.example\nnoproxy=second.example\n",
        Path::new(""),
    );
    assert_eq!(auth.no_proxy.as_deref(), Some("second.example"));

    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "noproxy=second.example\nno-proxy=first.example\n",
        Path::new(""),
    );
    assert_eq!(auth.no_proxy.as_deref(), Some("first.example"));
}

#[test]
fn cascade_https_proxy_uses_legacy_proxy_when_unset() {
    let auth = NpmrcAuth::from_ini::<NoEnv>("proxy=http://legacy.example:8080\n", Path::new(""));
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://legacy.example:8080"));
}

#[test]
fn cascade_explicit_https_proxy_wins_over_legacy_key() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "https-proxy=http://https.example:8080\nproxy=http://legacy.example:8080\n",
        Path::new(""),
    );
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://https.example:8080"));
}

#[test]
fn cascade_http_proxy_uses_resolved_https_proxy() {
    static_env!(
        EnvHttpButOverridden,
        &[("HTTP_PROXY", "http://env.example:80"), ("PROXY", "http://envproxy.example:80")]
    );
    let auth =
        NpmrcAuth::from_ini::<NoEnv>("https-proxy=http://https.example:8080\n", Path::new(""));
    let mut config = Config::new();
    auth.apply_to::<EnvHttpButOverridden>(&mut config);
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://https.example:8080"));
}

#[test]
fn cascade_no_proxy_true_literal_becomes_bypass_variant() {
    let auth = NpmrcAuth::from_ini::<NoEnv>("no-proxy=true\n", Path::new(""));
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.proxy.no_proxy, Some(NoProxySetting::Bypass));
}

#[test]
fn cascade_no_proxy_comma_list_trimmed() {
    let auth =
        NpmrcAuth::from_ini::<NoEnv>("no-proxy= foo.example , , bar.example ,\n", Path::new(""));
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config.proxy.no_proxy,
        Some(NoProxySetting::List(vec!["foo.example".to_string(), "bar.example".to_string()])),
    );
}

#[test]
fn cascade_legacy_proxy_false_disables_proxying_instead_of_falling_through() {
    static_env!(
        AllProxyEnvs,
        &[
            ("HTTPS_PROXY", "http://https-env.example:8080"),
            ("HTTP_PROXY", "http://http-env.example:8080"),
            ("PROXY", "http://bare-env.example:8080"),
        ]
    );
    let auth = NpmrcAuth::from_ini::<NoEnv>("proxy=false\n", Path::new(""));
    let mut config = Config::new();
    auth.apply_to::<AllProxyEnvs>(&mut config);
    assert_eq!(config.proxy.https_proxy, None);
    assert_eq!(config.proxy.http_proxy, None);
}

#[test]
fn cascade_https_proxy_key_wins_over_a_disabling_legacy_proxy() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "proxy=false\nhttps-proxy=http://https.example:8080\n",
        Path::new(""),
    );
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://https.example:8080"));
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://https.example:8080"));
}

#[test]
fn cascade_empty_legacy_proxy_key_falls_through_to_env() {
    static_env!(HttpsEnv, &[("HTTPS_PROXY", "http://https-env.example:8080")]);
    let auth = NpmrcAuth::from_ini::<NoEnv>("proxy=\n", Path::new(""));
    let mut config = Config::new();
    auth.apply_to::<HttpsEnv>(&mut config);
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://https-env.example:8080"));
}

#[test]
fn cascade_empty_https_proxy_key_falls_through_to_legacy_proxy_key() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "https-proxy=\nproxy=http://legacy.example:8080\n",
        Path::new(""),
    );
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://legacy.example:8080"));
}

#[test]
fn parses_inline_ca_from_ini() {
    let ini = format!("ca={}\n", TEST_CA_PEM.replace('\n', " "));
    // INI doesn't allow real newlines in values, but for round-trip
    // through this test we still parse `value` as a single line.
    let auth = NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new(""));
    assert_eq!(auth.ca.len(), 1, "auth.ca={:?}", auth.ca);
}

#[test]
fn parses_strict_ssl_true_and_false() {
    assert_eq!(
        NpmrcAuth::from_ini::<NoEnv>("strict-ssl=true\n", Path::new("")).strict_ssl,
        Some(true),
    );
    assert_eq!(
        NpmrcAuth::from_ini::<NoEnv>("strict-ssl=false\n", Path::new("")).strict_ssl,
        Some(false),
    );
}

#[test]
fn strict_ssl_invalid_value_silently_drops() {
    let auth = NpmrcAuth::from_ini::<NoEnv>("strict-ssl=maybe\n", Path::new(""));
    assert_eq!(auth.strict_ssl, None);
}

#[test]
fn strict_ssl_invalid_value_resets_prior_value() {
    // If the parser silently kept the earlier `false`, a typo on a later
    // line would leave TLS verification disabled — silently — until
    // the user noticed.
    let auth = NpmrcAuth::from_ini::<NoEnv>("strict-ssl=false\nstrict-ssl=oops\n", Path::new(""));
    assert_eq!(auth.strict_ssl, None);
}

#[test]
fn parses_local_address_from_ini() {
    let auth = NpmrcAuth::from_ini::<NoEnv>("local-address=10.0.0.5\n", Path::new(""));
    assert_eq!(auth.local_address.as_deref(), Some("10.0.0.5"));
}

#[test]
fn applies_local_address_parsed_as_ipaddr() {
    use std::net::Ipv4Addr;
    let auth =
        NpmrcAuth { local_address: Some("192.168.1.42".to_string()), ..NpmrcAuth::default() };
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.tls.local_address, Some(Ipv4Addr::new(192, 168, 1, 42).into()));
}

#[test]
fn invalid_local_address_silently_dropped() {
    // pnpm hands the value verbatim to undici and lets Node error at
    // connect time; pacquet validates early but errors silently per
    // the same parity policy as a missing `cafile`.
    let auth = NpmrcAuth { local_address: Some("not-an-ip".to_string()), ..NpmrcAuth::default() };
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.tls.local_address, None);
}

// Regression for <https://github.com/pnpm/pnpm/issues/14646>: a `ca=`
// whose `${VAR}` never resolved reaches the client builder as an empty
// entry, which the builder ignores.
#[test]
fn ca_with_an_unresolved_placeholder_still_builds_a_client() {
    let auth = NpmrcAuth::from_ini::<NoEnv>("ca=${CORP_CA}\n", Path::new(""));
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.tls.ca, vec![String::new()], "tls.ca={:?}", config.tls.ca);
    pnpm_network::ThrottledClient::for_installs(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &config.network_settings(),
    )
    .expect("an unresolved `ca` placeholder is ignored, not fatal");
}

// --- Per-registry TLS tests ---

#[test]
fn parses_scoped_inline_ca() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "//reg.example.com/:ca=-----BEGIN CERTIFICATE-----\\nMIIB-----END CERTIFICATE-----\n",
        Path::new(""),
    );
    let entry = auth.tls_by_uri.get("//reg.example.com/").expect("entry present");
    let ca = entry.ca.as_deref().expect("ca set");
    assert!(ca.contains('\n'), r"expected `\n` → newline expansion: {ca:?}");
    assert!(ca.contains("BEGIN CERTIFICATE"), "expected PEM header: {ca:?}");
}
