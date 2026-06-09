use std::path::Path;

use super::{EnvVar, NpmrcAuth, RawCreds, base64_decode, base64_encode};
use crate::Config;
use pacquet_network::NoProxySetting;
use pretty_assertions::assert_eq;

/// Generate a per-test unit struct implementing [`EnvVar`] from a
/// `&[(&str, &str)]` literal — saves each cascade test from spelling
/// out an `impl EnvVar` block. Avoids touching the real process
/// environment so cascade tests don't need
/// [`pacquet_testing_utils::env_guard::EnvGuard`]'s global lock.
macro_rules! static_env {
    ($name:ident, $entries:expr) => {
        struct $name;
        impl EnvVar for $name {
            fn var(name: &str) -> Option<String> {
                let entries: &[(&str, &str)] = $entries;
                entries.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_string())
            }
        }
    };
}

/// Test fake: the process environment is empty. Per the DI
/// pattern from
/// [pnpm/pacquet#339](https://github.com/pnpm/pacquet/issues/339),
/// the fake is a unit struct scoped to the test module; tests
/// turbofish it through the generic slot.
struct NoEnv;
impl EnvVar for NoEnv {
    fn var(_: &str) -> Option<String> {
        None
    }
}

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
fn ignores_non_auth_keys() {
    // These are all project-structural settings that pnpm 11 only reads
    // from pnpm-workspace.yaml now. Writing them to .npmrc should be a
    // no-op.
    //
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
fn ignores_comments_and_empty_lines() {
    let ini = "
# this is a comment
; another comment

registry=https://r.example
# trailing comment
";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.registry.as_deref(), Some("https://r.example"));
}

#[test]
fn ignores_malformed_lines() {
    let ini = "not_a_key_value\nregistry=https://r.example\n=orphan_equals\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.registry.as_deref(), Some("https://r.example"));
}

#[test]
fn parses_per_registry_auth_token() {
    let ini = "//npm.pkg.github.com/pnpm/:_authToken=ghp_xxx\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(
        auth.creds_by_uri
            .get("//npm.pkg.github.com/pnpm/")
            .map(|creds| creds.auth_token.as_deref()),
        Some(Some("ghp_xxx")),
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
    assert_eq!(
        auth.creds_by_uri.get("//reg.com/").map(|creds| creds.auth_token.as_deref()),
        Some(Some("abc123")),
    );
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

    assert!(auth.creds_by_uri.is_empty());
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

    assert!(auth.creds_by_uri.is_empty());
    assert!(
        auth.warnings
            .iter()
            .any(|warning| warning.contains("//registry.example.com/${SECRET}/:_authToken")),
    );
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

    assert!(auth.creds_by_uri.is_empty());
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
fn project_ini_keeps_literal_dollar_brace_fragments() {
    let auth = NpmrcAuth::from_project_ini::<NoEnv>(
        "//attacker.example/:_authToken=literal${token\n",
        Path::new(""),
    );

    assert_eq!(
        auth.creds_by_uri.get("//attacker.example/").map(|creds| creds.auth_token.as_deref()),
        Some(Some("literal${token")),
    );
    assert_eq!(auth.warnings, Vec::<String>::new());
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
    // Mirrors pnpm's `substituteEnv` lossy fallback: unresolved `${VAR}` becomes
    // "" so a downstream `Authorization: Bearer ...` header is never sent with a
    // literal placeholder. See <https://github.com/pnpm/pnpm/issues/11513>.
    let ini = "//reg.com/:_authToken=${MISSING}\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(
        auth.creds_by_uri.get("//reg.com/").map(|creds| creds.auth_token.as_deref()),
        Some(Some("")),
    );
    assert_eq!(auth.warnings.len(), 1);
    assert!(auth.warnings[0].contains("${MISSING}"));
}

#[test]
fn env_replace_failure_preserves_resolved_and_default_placeholders() {
    // Mixed value with one resolvable placeholder, one unresolved bare placeholder,
    // and one with a `:-default` fallback. Only the bare unresolved one becomes "";
    // the others must still expand. Guards against an earlier implementation that
    // stripped every `${...}` on any substitution failure.
    struct EnvWithSet;
    impl EnvVar for EnvWithSet {
        fn var(name: &str) -> Option<String> {
            (name == "SET").then(|| "AAA".to_owned())
        }
    }
    let ini = "//reg.com/:_authToken=${SET}-${UNSET}-${DEFAULTED:-fallback}\n";
    let auth = NpmrcAuth::from_ini::<EnvWithSet>(ini, Path::new(""));
    assert_eq!(
        auth.creds_by_uri.get("//reg.com/").map(|creds| creds.auth_token.as_deref()),
        Some(Some("AAA--fallback")),
    );
    assert_eq!(auth.warnings.len(), 1);
    assert!(auth.warnings[0].contains("${UNSET}"));
}

#[test]
fn basic_auth_built_from_username_and_password() {
    // Pnpm's `_password` is base64(raw_password). Header should
    // be `Basic base64(username:raw_password)`.
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
fn auth_pair_base64_passes_through_to_basic_header() {
    let pair = base64_encode("alice:p@ss");
    let ini = format!("//reg.com/:_auth={pair}\n");
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config.auth_headers.for_url("https://reg.com/").as_deref(),
        Some(format!("Basic {pair}").as_str()),
    );
}

/// `[section]`-style headers are not legal `.npmrc` syntax (npm's
/// rc files are flat key/value pairs). Smoke-test that they are
/// dropped silently. They fall through the no-`=` branch in
/// [`NpmrcAuth::from_ini`] so the parser never tries to interpret
/// them.
#[test]
fn ini_section_headers_are_dropped_silently() {
    let ini = "[default]\nregistry=https://r.example\n[other]\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.registry.as_deref(), Some("https://r.example"));
    assert_eq!(auth.warnings, Vec::<String>::new());
}

/// When a `${VAR}` placeholder appears in the *key* and cannot be
/// resolved, the parser substitutes it with "" and pushes a warning.
/// Mirrors `substituteEnv` in pnpm's `loadNpmrcFiles.ts`.
#[test]
fn env_replace_failure_on_key_warns_and_drops_unresolved_to_empty() {
    // `${MISSING}_authToken` resolves to the literal key `_authToken` (the
    // unresolved placeholder becomes ""), so it lands on `default_creds` as
    // the typed `_authToken` field. The point of this test is to exercise
    // the warning + lossy-substitution branch at the top of `from_ini`.
    let ini = "${MISSING}_authToken=abc\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.default_creds.auth_token.as_deref(), Some("abc"));
    assert!(auth.warnings.iter().any(|warning| warning.contains("${MISSING}")));
}

/// Top-level `_auth=`, `username=`, and `_password=` lines should
/// land on [`NpmrcAuth::default_creds`] so the resolved registry's
/// nerf-darted URI gets a `Basic` header.
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

/// A `//host/:_password=…` line on its own (no matching `username`)
/// produces no `Basic` header. The credential shape needs both
/// halves. Hits the `None` fallthrough in [`creds_to_header`].
#[test]
fn lone_per_registry_password_produces_no_header() {
    let ini = format!("//reg.com/:_password={}\n", base64_encode("solo"));
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(config.auth_headers.for_url("https://reg.com/"), None);
}

/// Per-registry creds with a recognisable suffix should be carried
/// through [`NpmrcAuth::build_auth_headers`] and surface as a
/// `Basic` header for matching URLs. Exercises the
/// `auth_header_by_uri.insert(...)` branch.
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

/// `//host/:somethingUnknown=value` lines are dropped silently.
/// [`split_creds_key`] returns `None` for anything outside
/// [`CREDS_SUFFIXES`], and the line then falls through to
/// [`apply_creds_field`] on [`NpmrcAuth::default_creds`] with a
/// non-matching field. Exercises both no-match arms.
#[test]
fn unknown_per_registry_suffix_is_silently_dropped() {
    let ini = "//reg.example/:registry=https://other.example/\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert!(auth.creds_by_uri.is_empty());
    assert_eq!(auth.default_creds, RawCreds::default());
    assert_eq!(auth.warnings, Vec::<String>::new());
}

/// [`NpmrcAuth::apply_registry_and_warn`] should drain the warning
/// queue. Pnpm's `substituteEnv` writes the same string to stderr
/// via `globalWarn` once per resolution failure.
#[test]
fn apply_registry_and_warn_drains_warnings() {
    let ini = "//reg.com/:_authToken=${MISSING}\n";
    let mut auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.warnings.len(), 1);
    let mut config = Config::new();
    auth.apply_registry_and_warn(&mut config);
    assert!(auth.warnings.is_empty(), "warnings should be drained after flush");
}

/// When `_password` is *not* valid base64, [`creds_to_header`]
/// falls back to using the raw string verbatim. Mirrors the
/// `unwrap_or_else` branch inside that function. Pnpm's
/// `parseBasicAuth` doesn't have this exact fallback (it always
/// `atob`s), but pacquet's tolerance avoids losing the credential
/// for `.npmrc` files where `_password` was already a raw value.
#[test]
fn invalid_base64_password_falls_back_to_raw_value() {
    // `*` is outside the base64 alphabet, so `base64_decode`
    // returns `None` and the raw string is used as the password.
    let ini = "//reg.com/:username=alice\n//reg.com/:_password=raw*pw\n";
    let mut config = Config::new();
    NpmrcAuth::from_ini::<NoEnv>(ini, Path::new("")).apply_to::<NoEnv>(&mut config);
    assert_eq!(
        config.auth_headers.for_url("https://reg.com/").as_deref(),
        Some(format!("Basic {}", base64_encode("alice:raw*pw")).as_str()),
    );
}

/// Exercises every branch of [`base64_decode`]: the alphanumeric
/// arms, the `+` arm, the `/` arm, the `=` padding break, and the
/// "invalid character" return. Without these the password-decode
/// fallback (`unwrap_or_else(... pass_b64.clone())`) path stays
/// unreachable from the parser tests.
#[test]
fn base64_decode_covers_every_alphabet_branch() {
    // Standard alphanumeric round-trip.
    assert_eq!(base64_decode(&base64_encode("alice:hunter2")).as_deref(), Some("alice:hunter2"));
    // `/` arm: `"???"` (three 0x3f bytes) encodes to `"Pz8/"`.
    assert_eq!(base64_decode("Pz8/").as_deref(), Some("???"));
    // `+` arm: `"~~~"` (three 0x7e bytes) encodes to `"fn5+"`.
    assert_eq!(base64_decode("fn5+").as_deref(), Some("~~~"));
    // `=` padding short-circuits the loop on a 2-byte input.
    assert_eq!(base64_decode("aGk=").as_deref(), Some("hi"));
    // Redundant trailing padding is ignored, matching pnpm's tolerant
    // credential decoder.
    assert_eq!(base64_decode("aGk===").as_deref(), Some("hi"));
    // Invalid byte returns None so the parser keeps the raw
    // value verbatim. `*` is not in the alphabet.
    assert_eq!(base64_decode("not*base64"), None);
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
    // pnpm pipes both spellings into a single `noProxy` slot — the last
    // assignment in `.npmrc` order wins, same as upstream's single field.
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
    // Mirrors upstream: `httpsProxy ?? proxy ?? env`.
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
    // pnpm: `httpProxy ?? httpsProxy ?? env(HTTP_PROXY) ?? env(PROXY)`.
    // With only `https-proxy` set the http side inherits it — *and* the
    // env vars are not consulted.
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
    // When neither `.npmrc` nor `https-proxy` is set, http falls through
    // `HTTP_PROXY` first, then the bare `PROXY` env.
    static_env!(BareProxy, &[("PROXY", "http://barenv.example:80")]);
    let auth = NpmrcAuth::default();
    let mut config = Config::new();
    auth.apply_to::<BareProxy>(&mut config);
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://barenv.example:80"));
    assert_eq!(config.proxy.https_proxy, None);
}

#[test]
fn cascade_env_var_lowercase_lookup() {
    // Upstream tries upper then lower case. With only the lowercase env
    // populated, the lookup must still find it.
    static_env!(LowercaseEnv, &[("https_proxy", "http://lower.example:8080")]);
    let auth = NpmrcAuth::default();
    let mut config = Config::new();
    auth.apply_to::<LowercaseEnv>(&mut config);
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://lower.example:8080"));
}

// --- TLS + local-address tests ---

/// Same self-signed cert as `crates/network/src/tests.rs`; loaded from
/// the shared fixture under `crates/network/tests/fixtures/test-ca.pem`
/// so the base64 body stays out of the typos linter's dictionary.
const TEST_CA_PEM: &str = include_str!("../../../network/tests/fixtures/test-ca.pem");

#[test]
fn parses_inline_ca_from_ini() {
    let ini = format!("ca={}\n", TEST_CA_PEM.replace('\n', " "));
    // INI doesn't allow real newlines in values, but for round-trip
    // through this test we still parse `value` as a single line. The
    // important assertion is that the value lands on `auth.ca`.
    let auth = NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new(""));
    assert_eq!(auth.ca.len(), 1, "auth.ca={:?}", auth.ca);
}

#[test]
fn parses_cafile_path_from_ini() {
    let auth = NpmrcAuth::from_ini::<NoEnv>("cafile=/etc/pacquet/ca.pem\n", Path::new(""));
    assert_eq!(auth.cafile.as_deref(), Some("/etc/pacquet/ca.pem"));
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
fn cafile_absolute_path_passes_through_unchanged() {
    let npmrc_dir = tempfile::tempdir().expect("tempdir");
    let abs_cafile = tempfile::NamedTempFile::new().expect("tempfile");
    let abs_path = abs_cafile.path().to_string_lossy().into_owned();
    let auth = NpmrcAuth::from_ini::<NoEnv>(&format!("cafile={abs_path}\n"), npmrc_dir.path());
    assert_eq!(auth.cafile.as_deref(), Some(abs_path.as_str()));
}

#[test]
fn cafile_empty_value_passes_through_unchanged() {
    // An explicit `cafile=` (empty) means "no cafile". Joining the
    // npmrc dir onto an empty path would incorrectly load the dir
    // itself, so empty must short-circuit.
    let npmrc_dir = tempfile::tempdir().expect("tempdir");
    let auth = NpmrcAuth::from_ini::<NoEnv>("cafile=\n", npmrc_dir.path());
    assert_eq!(auth.cafile.as_deref(), Some(""));
}

// End-to-end regression for <https://github.com/pnpm/pnpm/issues/11624>.
#[test]
fn cafile_relative_path_loads_ca_from_disk_via_apply() {
    use std::io::Write;
    let npmrc_dir = tempfile::tempdir().expect("tempdir");
    let certs_dir = npmrc_dir.path().join("certs");
    std::fs::create_dir_all(&certs_dir).expect("certs dir");
    let mut ca_file = std::fs::File::create(certs_dir.join("ca.pem")).expect("create ca.pem");
    ca_file.write_all(TEST_CA_PEM.as_bytes()).expect("write");
    let auth = NpmrcAuth::from_ini::<NoEnv>("cafile=certs/ca.pem\n", npmrc_dir.path());
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.tls.ca.len(), 1, "tls.ca={:?}", config.tls.ca);
    assert!(config.tls.ca[0].contains("BEGIN CERTIFICATE"));
}

#[test]
fn parses_cert_and_key_from_ini() {
    let ini = "cert=cert-pem\nkey=key-pem\n";
    let auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.cert.as_deref(), Some("cert-pem"));
    assert_eq!(auth.key.as_deref(), Some("key-pem"));
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
    // pnpm/nopt drops non-boolean values. Pacquet does the same so
    // the per-emit-site `?? true` default kicks in.
    let auth = NpmrcAuth::from_ini::<NoEnv>("strict-ssl=maybe\n", Path::new(""));
    assert_eq!(auth.strict_ssl, None);
}

#[test]
fn strict_ssl_invalid_value_resets_prior_value() {
    // A later invalid `strict-ssl=` line resets the slot to `None`
    // so the build-site `unwrap_or(true)` default kicks in. If the
    // parser silently kept the earlier `false`, a typo on a later
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
fn applies_inline_ca_to_config() {
    let auth = NpmrcAuth { ca: vec![TEST_CA_PEM.to_string()], ..NpmrcAuth::default() };
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.tls.ca.len(), 1);
    let first = &config.tls.ca[0];
    assert!(first.contains("BEGIN CERTIFICATE"), "inline CA missing header: {first:?}");
}

/// `strict-ssl` stays a top-level toggle, but unscoped `cert`/`key`
/// are rescoped to the source's registry (the npmjs default here, since
/// no `registry=` is set) — matching pnpm's `rescopeUnscopedCreds`,
/// which pins client identity per registry rather than sending it to
/// every host.
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

#[test]
fn cafile_reads_and_splits_into_per_cert_pems() {
    use std::io::Write;
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    // Two certs concatenated — same as a real-world multi-CA bundle.
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
fn cafile_trailing_garbage_is_preserved_for_downstream_parser() {
    // Mirrors pnpm's `loadCAFile`: a non-empty chunk after the final
    // `-----END CERTIFICATE-----` gets the delimiter re-appended
    // (producing a malformed PEM) so downstream
    // `Certificate::from_pem` surfaces the parse error. Silently
    // dropping the trailing chunk would mask a truncated cert
    // bundle and leave the user wondering why their CA list is
    // shorter than expected.
    use std::io::Write;
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let bundle = format!("{TEST_CA_PEM}\ngarbage-not-a-cert");
    tmp.as_file().write_all(bundle.as_bytes()).expect("write bundle");
    let auth = NpmrcAuth {
        cafile: Some(tmp.path().to_string_lossy().into_owned()),
        ..NpmrcAuth::default()
    };
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert_eq!(config.tls.ca.len(), 2, "tls.ca={:?}", config.tls.ca);
    assert!(
        config.tls.ca[1].starts_with("garbage-not-a-cert"),
        "trailing garbage entry was not preserved: {:?}",
        config.tls.ca[1],
    );
    assert!(
        config.tls.ca[1].ends_with("-----END CERTIFICATE-----"),
        "delimiter was not re-appended to garbage entry: {:?}",
        config.tls.ca[1],
    );
}

#[test]
fn cafile_not_found_is_silently_treated_as_unset() {
    let auth = NpmrcAuth {
        cafile: Some("/nonexistent/path/to/ca.pem".to_string()),
        ..NpmrcAuth::default()
    };
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert!(config.tls.ca.is_empty(), "missing cafile must not produce CAs");
}

#[test]
fn inline_ca_and_cafile_concatenate() {
    use std::io::Write;
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    tmp.as_file().write_all(TEST_CA_PEM.as_bytes()).expect("write");
    let auth = NpmrcAuth {
        ca: vec![TEST_CA_PEM.to_string()],
        cafile: Some(tmp.path().to_string_lossy().into_owned()),
        ..NpmrcAuth::default()
    };
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    // Inline first, cafile second — same ordering pnpm produces.
    assert_eq!(config.tls.ca.len(), 2, "tls.ca={:?}", config.tls.ca);
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

// --- Per-registry TLS tests ---

#[test]
fn parses_scoped_inline_ca() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "//reg.example.com/:ca=-----BEGIN CERTIFICATE-----\\nMIIB-----END CERTIFICATE-----\n",
        Path::new(""),
    );
    let entry = auth.tls_by_uri.get("//reg.example.com/").expect("entry present");
    let ca = entry.ca.as_deref().expect("ca set");
    assert!(ca.contains('\n'), "expected `\\n` → newline expansion: {ca:?}");
    assert!(ca.contains("BEGIN CERTIFICATE"), "expected PEM header: {ca:?}");
}

#[test]
fn parses_scoped_cafile_reads_from_disk() {
    use std::io::Write;
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    tmp.as_file().write_all(TEST_CA_PEM.as_bytes()).expect("write");
    let ini = format!("//reg.example.com/:cafile={}\n", tmp.path().display());
    let auth = NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new(""));
    let entry = auth.tls_by_uri.get("//reg.example.com/").expect("entry present");
    let ca = entry.ca.as_deref().expect("ca set");
    assert!(ca.contains("BEGIN CERTIFICATE"), "expected PEM contents from cafile: {ca:?}");
}

#[test]
fn parses_scoped_cafile_missing_silently_dropped() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "//reg.example.com/:cafile=/nonexistent/path/ca.pem\n",
        Path::new(""),
    );
    // Either the entry doesn't exist, or it exists with `ca = None`.
    // `PerRegistryTls::from_map` filters all-`None` entries later;
    // here the parse-time behavior is "no entry written".
    assert!(
        auth.tls_by_uri.get("//reg.example.com/").is_none_or(|entry| entry.ca.is_none()),
        "missing cafile must not produce a non-None ca slot: {:?}",
        auth.tls_by_uri,
    );
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
fn scoped_inline_and_file_share_same_slot_last_wins() {
    // pnpm writes `:cert` and `:certfile` to the same `tls.cert`
    // slot. The last assignment in the .npmrc wins.
    use std::io::Write;
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    tmp.as_file().write_all(b"FROM-FILE").expect("write");
    let ini = format!(
        "//reg.example.com/:cert=inline\n//reg.example.com/:certfile={}\n",
        tmp.path().display(),
    );
    let auth = NpmrcAuth::from_ini::<NoEnv>(&ini, Path::new(""));
    let entry = auth.tls_by_uri.get("//reg.example.com/").expect("entry present");
    assert_eq!(entry.cert.as_deref(), Some("FROM-FILE"));
}

#[test]
fn scoped_n_escape_expansion_only_on_inline() {
    // pnpm's `:ca=...` value goes through `.replace(/\\n/g, '\n')`.
    // The `:cafile` variant reads from disk and doesn't apply the
    // replacement (the file already has real newlines).
    let auth = NpmrcAuth::from_ini::<NoEnv>("//reg.example.com/:ca=line1\\nline2\n", Path::new(""));
    let entry = auth.tls_by_uri.get("//reg.example.com/").expect("entry present");
    assert_eq!(entry.ca.as_deref(), Some("line1\nline2"));
}

#[test]
fn applies_tls_by_uri_to_config_drops_empty() {
    let auth = NpmrcAuth::from_ini::<NoEnv>(
        "//keep.example.com/:ca=ca-pem\n//drop.example.com/:registry=https://drop.example/\n",
        Path::new(""),
    );
    // `//drop.example.com/:registry=` doesn't match any TLS suffix
    // so no `RegistryTls` entry is ever created for that prefix.
    let mut config = Config::new();
    auth.apply_to::<NoEnv>(&mut config);
    assert!(config.tls_by_uri.get("//keep.example.com/").is_some(), "non-empty entry kept");
    assert!(config.tls_by_uri.get("//drop.example.com/").is_none(), "non-TLS key ignored");
}

#[test]
fn scoped_tls_keys_dont_collide_with_top_level() {
    // Top-level `ca=`, `cert=`, `key=`, `cafile=` arms run *before*
    // the SSL-suffix matcher. A bare `ca=` line should land on
    // `auth.ca`, not in `tls_by_uri` as registry=`""`.
    let auth = NpmrcAuth::from_ini::<NoEnv>("ca=top-level\n", Path::new(""));
    assert_eq!(auth.ca, vec!["top-level".to_string()]);
    assert!(auth.tls_by_uri.is_empty(), "top-level `ca=` must not pollute tls_by_uri");
}
