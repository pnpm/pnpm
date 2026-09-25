use super::{discover_os_proxy, parse_macos_scutil_proxy, parse_windows_internet_settings};

#[test]
fn discover_os_proxy_stays_empty_in_unit_tests() {
    assert_eq!(discover_os_proxy(), super::OsProxy::default());
}

#[test]
fn windows_proxy_disabled_or_missing_ignores_the_server() {
    let disabled = "\
HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings
    ProxyEnable    REG_DWORD    0x0
    ProxyServer    REG_SZ    proxy.example:8080
";
    assert_eq!(parse_windows_internet_settings(disabled), super::OsProxy::default());

    let missing = "\
HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings
    ProxyServer    REG_SZ    proxy.example:8080
";
    assert_eq!(parse_windows_internet_settings(missing), super::OsProxy::default());
}

#[test]
fn windows_bare_server_applies_to_both_schemes() {
    let output = "\
HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings
    ProxyEnable    REG_DWORD    0x1
    ProxyServer    REG_SZ    proxy.example:8080
    ProxyOverride    REG_SZ    localhost;<local>;*.corp.example;
";
    let parsed = parse_windows_internet_settings(output);
    assert_eq!(parsed.http.as_deref(), Some("proxy.example:8080"));
    assert_eq!(parsed.https.as_deref(), Some("proxy.example:8080"));
    assert_eq!(parsed.bypass.as_deref(), Some("localhost,corp.example"));
}

#[test]
fn windows_per_scheme_server_keeps_http_and_https_apart() {
    let output = "    ProxyEnable    REG_DWORD    0x00000001\n    \
ProxyServer    REG_SZ    http=10.1.1.1:80;https=10.1.1.2:443;socks=10.1.1.3:1080\n";
    let parsed = parse_windows_internet_settings(output);
    assert_eq!(parsed.http.as_deref(), Some("10.1.1.1:80"));
    assert_eq!(parsed.https.as_deref(), Some("10.1.1.2:443"));
    assert_eq!(parsed.bypass, None);
}

#[test]
fn macos_enable_flags_and_exceptions() {
    let output = "\
<dictionary> {
  ExceptionsList : <array> {
    0 : *.local
    1 : <local>
    2 : example.com
  }
  HTTPEnable : 1
  HTTPPort : 8080
  HTTPProxy : 10.0.0.1
  HTTPSEnable : 0
  HTTPSPort : 8443
  HTTPSProxy : 10.0.0.2
}
";
    let parsed = parse_macos_scutil_proxy(output);
    assert_eq!(parsed.http.as_deref(), Some("10.0.0.1:8080"));
    assert_eq!(parsed.https, None);
    assert_eq!(parsed.bypass.as_deref(), Some("local,example.com"));
}

#[test]
fn macos_https_without_a_port_uses_the_host() {
    let output = "\
  HTTPSEnable : 1
  HTTPSProxy : proxy.example
  HTTPEnable : 0
  HTTPProxy : ignored.example
";
    let parsed = parse_macos_scutil_proxy(output);
    assert_eq!(parsed.https.as_deref(), Some("proxy.example"));
    assert_eq!(parsed.http, None);
}
