use crate::RouteContext;
use pnpr_config::Config;
use pnpr_fixtures::MockConfigPresets;
use std::path::PathBuf;

#[test]
fn local_package_uses_the_serving_path() {
    let mut config = Config::proxy("127.0.0.1:7677".parse().unwrap(), PathBuf::from("unused"));
    config.http.public_url = "https://registry.example/base/".to_string();
    let context = RouteContext::from_config(&config);
    for (path, package) in [
        ("npm/widget", "widget"),
        ("npm/~internal/widget/-/widget-1.0.0.tgz?download=1", "widget"),
        ("npm/@scope/widget", "@scope/widget"),
        ("npm/~internal/@scope%2Fwidget@1.0.0+r0", "@scope/widget"),
        ("npm/~internal/@scope/widget/-/widget-1.0.0.tgz", "@scope/widget"),
    ] {
        let url = format!("{}{path}", config.http.public_url);
        assert_eq!(context.local_package(&url).as_deref(), Some(package), "{url}");
    }
    assert_eq!(context.local_package("https://registry.example/base-other/npm/widget"), None);
    assert_eq!(context.local_package("https://other.example/base/npm/widget"), None);
}
