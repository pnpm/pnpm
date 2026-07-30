use std::{collections::BTreeMap, sync::Arc};

use pacquet_config::Config;
use pacquet_network::{AuthHeaders, AuthHeadersByScope, NoProxySetting};
use pretty_assertions::assert_eq;

use super::{import_method_name, project_config};

fn config_with_auth(by_scope: AuthHeadersByScope) -> Config {
    let mut config = Config::default();
    config.registry = "https://reg.example/npm/".to_string();
    config.registries =
        BTreeMap::from([("@scope".to_string(), "https://reg.example/scoped/".to_string())]);
    config.auth_headers = Arc::new(AuthHeaders::from_by_scope(by_scope));
    config
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

    let scope = resolved.registries.iter().find(|r| r.name == "@scope").expect("@scope registry");
    assert_eq!(scope.auth_header.as_deref(), Some("Bearer scoped-token"));

    let mut config = config;
    config.auth_headers = Arc::new(AuthHeaders::from_by_scope(AuthHeadersByScope::from([(
        "//reg.example/scoped/".to_string(),
        BTreeMap::from([("@".to_string(), "Bearer registry-wide".to_string())]),
    )])));
    let resolved = project_config(&config);
    let scope = resolved.registries.iter().find(|r| r.name == "@scope").expect("@scope registry");
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

#[test]
fn import_method_names_match_the_install_option_strings() {
    use pacquet_config::PackageImportMethod;
    for (method, name) in [
        (PackageImportMethod::Auto, "auto"),
        (PackageImportMethod::Hardlink, "hardlink"),
        (PackageImportMethod::Copy, "copy"),
        (PackageImportMethod::Clone, "clone"),
        (PackageImportMethod::CloneOrCopy, "clone-or-copy"),
    ] {
        assert_eq!(import_method_name(method), name);
    }
}
