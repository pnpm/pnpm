use std::{collections::BTreeMap, sync::Arc};

use pnpm_config::Config;
use pnpm_network::{AuthHeaders, AuthHeadersByScope, NoProxySetting};
use pretty_assertions::assert_eq;

use super::{import_method_name, project_config};

fn config_with_auth(by_scope: AuthHeadersByScope) -> Config {
    Config {
        registry: "https://reg.example/npm/".to_string(),
        registries_by_scope: BTreeMap::from([(
            "@scope".to_string(),
            "https://reg.example/scoped/".to_string(),
        )]),
        auth_headers: Arc::new(AuthHeaders::from_by_scope(by_scope)),
        ..Config::default()
    }
}

#[test]
fn registries_carry_their_static_auth_headers() {
    let config = config_with_auth(AuthHeadersByScope::from([
        (
            "//reg.example/npm/".to_string(),
            BTreeMap::from([("@".to_string(), "Bearer default-token".to_string())]),
        ),
        (
            "//reg.example/scoped/".to_string(),
            BTreeMap::from([("@scope".to_string(), "Bearer scoped-token".to_string())]),
        ),
    ]));

    let resolved = project_config(&config);

    let by_name: BTreeMap<&str, Option<&str>> = resolved
        .registries
        .iter()
        .map(|registry| (registry.name.as_str(), registry.auth_header.as_deref()))
        .collect();
    assert_eq!(by_name["default"], Some("Bearer default-token"));
    assert_eq!(by_name["@scope"], Some("Bearer scoped-token"));
    assert_eq!(
        resolved.auth_header_by_uri.get("//reg.example/npm/").map(String::as_str),
        Some("Bearer default-token"),
        "registry-wide headers land in the uri-keyed map",
    );
    assert_eq!(
        resolved.auth_header_by_uri.get("//reg.example/scoped/"),
        None,
        "scope-keyed credentials stay off the registry-wide map",
    );
}

/// A scope registry with both a scope-keyed and a registry-wide credential
/// at its URI must pick its own scope's; a scope registry with only the
/// registry-wide one falls back to it.
#[test]
fn scope_registry_prefers_its_scope_credential() {
    let config = config_with_auth(AuthHeadersByScope::from([(
        "//reg.example/scoped/".to_string(),
        BTreeMap::from([
            ("@".to_string(), "Bearer registry-wide".to_string()),
            ("@scope".to_string(), "Bearer scoped-token".to_string()),
        ]),
    )]));

    let resolved = project_config(&config);

    let scope = resolved
        .registries
        .iter()
        .find(|registry| registry.name == "@scope")
        .expect("@scope registry");
    assert_eq!(scope.auth_header.as_deref(), Some("Bearer scoped-token"));

    let mut config = config;
    config.auth_headers = Arc::new(AuthHeaders::from_by_scope(AuthHeadersByScope::from([(
        "//reg.example/scoped/".to_string(),
        BTreeMap::from([("@".to_string(), "Bearer registry-wide".to_string())]),
    )])));
    let resolved = project_config(&config);
    let scope = resolved
        .registries
        .iter()
        .find(|registry| registry.name == "@scope")
        .expect("@scope registry");
    assert_eq!(scope.auth_header.as_deref(), Some("Bearer registry-wide"));
}

#[test]
fn no_proxy_projects_bypass_as_true_and_hosts_as_a_joined_string() {
    let mut config = Config::default();
    config.proxy.no_proxy = Some(NoProxySetting::Bypass);
    assert_eq!(project_config(&config).no_proxy, Some(serde_json::Value::Bool(true)));

    config.proxy.no_proxy =
        Some(NoProxySetting::List(vec!["a.example".to_string(), "b.example".to_string()]));

    assert_eq!(
        project_config(&config).no_proxy,
        Some(serde_json::Value::String("a.example,b.example".to_string())),
    );
}

#[test]
fn empty_ca_projects_as_absent() {
    let config = Config::default();
    assert_eq!(project_config(&config).ca, None);
}

/// Every projected import-method name must be accepted back by the
/// `install` option parser, so an embedder can feed `readConfig`'s value
/// into `install` without silently losing the configured behavior.
#[test]
fn import_method_names_round_trip_through_the_install_parser() {
    use pnpm_config::PackageImportMethod;
    for (method, name) in [
        (PackageImportMethod::Auto, "auto"),
        (PackageImportMethod::Hardlink, "hardlink"),
        (PackageImportMethod::Copy, "copy"),
        (PackageImportMethod::Clone, "clone"),
        (PackageImportMethod::CloneOrCopy, "clone-or-copy"),
    ] {
        assert_eq!(import_method_name(method), name);
        assert_eq!(crate::install::parse_import_method(name), Some(method));
    }
}

/// End-to-end through the real resolver: a project `.npmrc` cascade must
/// surface in the projection. Asserts only the fixture's own keys — the
/// process environment and user config may add entries, never remove
/// these.
#[test]
fn read_config_resolves_the_project_npmrc_cascade() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".npmrc"),
        concat!(
            "@fixture:registry=https://reg.fixture.example/scoped/\n",
            "//reg.fixture.example/scoped/:_authToken=fixture-token\n",
            "https-proxy=http://proxy.fixture.example:8080\n",
            "no-proxy=internal.fixture.example\n",
            "strict-ssl=false\n",
        ),
    )
    .expect("write .npmrc");

    let resolved =
        super::read_config(super::ReadConfigOptions { dir: dir.path().display().to_string() })
            .expect("read config");

    let fixture_registry = resolved
        .registries
        .iter()
        .find(|registry| registry.name == "@fixture")
        .expect("@fixture registry resolved from the project .npmrc");
    assert_eq!(fixture_registry.url, "https://reg.fixture.example/scoped/");
    assert_eq!(fixture_registry.auth_header.as_deref(), Some("Bearer fixture-token"));
    assert_eq!(
        resolved.auth_header_by_uri.get("//reg.fixture.example/scoped/").map(String::as_str),
        Some("Bearer fixture-token"),
    );
    assert_eq!(resolved.https_proxy.as_deref(), Some("http://proxy.fixture.example:8080"));
    assert_eq!(
        resolved.no_proxy,
        Some(serde_json::Value::String("internal.fixture.example".to_string())),
    );
    assert_eq!(resolved.strict_ssl, Some(false));
    assert!(!resolved.store_dir.is_empty());
    assert!(!resolved.cache_dir.is_empty());
}

/// A `pnpm-workspace.yaml` setting must surface both as its resolved
/// value and as an entry in `explicit_settings`, while an unset sibling
/// (whose projected value is an engine default) stays off that list.
#[test]
fn read_config_reports_explicitly_set_workspace_settings() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("pnpm-workspace.yaml"),
        "fetchRetries: 7\nfetchWarnTimeoutMs: 2345\nfetchMinSpeedKiBps: 12\n",
    )
    .expect("write pnpm-workspace.yaml");

    let resolved =
        super::read_config(super::ReadConfigOptions { dir: dir.path().display().to_string() })
            .expect("read config");

    assert_eq!(resolved.fetch_retries, 7);
    assert_eq!(resolved.fetch_warn_timeout_ms, 2_345);
    assert_eq!(resolved.fetch_min_speed_ki_bps, 12);
    assert!(resolved.explicit_settings.contains(&"fetchRetries".to_string()));
    assert!(resolved.explicit_settings.contains(&"fetchWarnTimeoutMs".to_string()));
    assert!(resolved.explicit_settings.contains(&"fetchMinSpeedKiBps".to_string()));
    assert!(!resolved.explicit_settings.contains(&"fetchTimeout".to_string()));
}
