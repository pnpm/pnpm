use pnpm_network::NoProxySetting;
use pretty_assertions::assert_eq;

use super::{ProxyEnv, ProxyKeys, ProxyValue};
use crate::os_proxy::OsProxy;

fn os_proxy() -> OsProxy {
    OsProxy {
        https: Some("http://os-https.example:9".to_string()),
        http: Some("http://os-http.example:9".to_string()),
        bypass: Some("os.example".to_string()),
    }
}

#[test]
fn env_https_proxy_wins_over_the_operating_system() {
    let resolved = ProxyKeys {
        env: ProxyEnv {
            https_proxy: Some("http://env.example:8080".to_string()),
            ..ProxyEnv::default()
        },
        os: os_proxy(),
        ..ProxyKeys::default()
    }
    .resolve();
    assert_eq!(resolved.https_proxy.as_deref(), Some("http://env.example:8080"));
    assert_eq!(resolved.http_proxy.as_deref(), Some("http://env.example:8080"));
    assert_eq!(resolved.no_proxy, Some(NoProxySetting::List(vec!["os.example".to_string()])));
}

#[test]
fn config_proxy_wins_over_env_and_the_operating_system() {
    let resolved = ProxyKeys {
        https_proxy: ProxyValue::Url("http://npmrc.example:8080".to_string()),
        env: ProxyEnv {
            https_proxy: Some("http://env.example:8080".to_string()),
            ..ProxyEnv::default()
        },
        os: os_proxy(),
        ..ProxyKeys::default()
    }
    .resolve();
    assert_eq!(resolved.https_proxy.as_deref(), Some("http://npmrc.example:8080"));
    assert_eq!(resolved.http_proxy.as_deref(), Some("http://npmrc.example:8080"));
}

#[test]
fn disabled_legacy_proxy_does_not_fall_through_to_the_operating_system() {
    let resolved =
        ProxyKeys { legacy_proxy: ProxyValue::Disabled, os: os_proxy(), ..ProxyKeys::default() }
            .resolve();
    assert_eq!(resolved.https_proxy, None);
    assert_eq!(resolved.http_proxy, None);
}

#[test]
fn empty_env_proxy_shadows_the_operating_system() {
    let resolved = ProxyKeys {
        env: ProxyEnv {
            https_proxy: Some(String::new()),
            no_proxy: Some(String::new()),
            ..ProxyEnv::default()
        },
        os: os_proxy(),
        ..ProxyKeys::default()
    }
    .resolve();
    assert_eq!(resolved.https_proxy.as_deref(), Some(""));
    assert_eq!(resolved.http_proxy.as_deref(), Some(""));
    assert_eq!(resolved.no_proxy, Some(NoProxySetting::List(vec![])));
}

#[test]
fn operating_system_proxy_fills_slots_left_unset() {
    let both = ProxyKeys { os: os_proxy(), ..ProxyKeys::default() }.resolve();
    assert_eq!(both.https_proxy.as_deref(), Some("http://os-https.example:9"));
    assert_eq!(both.http_proxy.as_deref(), Some("http://os-http.example:9"));
    assert_eq!(both.no_proxy, Some(NoProxySetting::List(vec!["os.example".to_string()])));

    let https_only = ProxyKeys {
        os: OsProxy { https: Some("http://os-https.example:9".to_string()), ..OsProxy::default() },
        ..ProxyKeys::default()
    }
    .resolve();
    assert_eq!(https_only.https_proxy.as_deref(), Some("http://os-https.example:9"));
    assert_eq!(https_only.http_proxy.as_deref(), Some("http://os-https.example:9"));

    let http_only = ProxyKeys {
        os: OsProxy { http: Some("http://os-http.example:9".to_string()), ..OsProxy::default() },
        ..ProxyKeys::default()
    }
    .resolve();
    assert_eq!(http_only.https_proxy, None);
    assert_eq!(http_only.http_proxy.as_deref(), Some("http://os-http.example:9"));
}

#[test]
fn env_no_proxy_wins_over_the_operating_system() {
    let resolved = ProxyKeys {
        env: ProxyEnv { no_proxy: Some("env.example".to_string()), ..ProxyEnv::default() },
        os: os_proxy(),
        ..ProxyKeys::default()
    }
    .resolve();
    assert_eq!(resolved.no_proxy, Some(NoProxySetting::List(vec!["env.example".to_string()])));
}

#[test]
fn env_http_proxy_is_kept_when_only_https_comes_from_the_operating_system() {
    let resolved = ProxyKeys {
        env: ProxyEnv {
            http_proxy: Some("http://env-http.example:8080".to_string()),
            ..ProxyEnv::default()
        },
        os: os_proxy(),
        ..ProxyKeys::default()
    }
    .resolve();
    assert_eq!(resolved.https_proxy.as_deref(), Some("http://os-https.example:9"));
    assert_eq!(resolved.http_proxy.as_deref(), Some("http://env-http.example:8080"));
}
