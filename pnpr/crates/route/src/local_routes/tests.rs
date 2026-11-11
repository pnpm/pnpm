use crate::RouteContext;
use pnpr_config::Config;
use pnpr_fixtures::MockConfigPresets;
use std::path::PathBuf;

#[test]
fn local_package_uses_the_serving_path() {
    let mut config = Config::proxy("127.0.0.1:7677".parse().unwrap(), PathBuf::from("unused"));
    config.http.public_url = "https://registry.example/base".to_string();
    let context = RouteContext::from_config(&config);
    for (path, package) in [
        ("npm/widget", "widget"),
        ("npm/~internal/widget/-/widget-1.0.0.tgz?download=1", "widget"),
        ("npm/@scope/widget", "@scope/widget"),
        ("npm/~internal/@scope%2Fwidget@1.0.0+r0", "@scope/widget"),
        ("npm/~internal/@scope/widget/-/widget-1.0.0.tgz", "@scope/widget"),
    ] {
        let url = format!("{}/{path}", config.http.public_url);
        assert_eq!(context.local_package(&url).as_deref(), Some(package), "{url}");
    }
    assert_eq!(context.local_package("https://registry.example/base-other/npm/widget"), None);
    assert_eq!(context.local_package("https://other.example/base/npm/widget"), None);
}

#[test]
fn local_fetch_rejects_urls_that_classification_does_not_recognize() {
    use crate::{Footprint, RouteHook};
    use pnpm_network::UpstreamRouteHook;
    use pnpr_policy::Identity;
    use std::sync::{Arc, Mutex};

    let mut config = Config::proxy("127.0.0.1:7677".parse().unwrap(), PathBuf::from("unused"));
    config.http.public_url = "https://registry.example/base".to_string();
    let context = RouteContext::from_config(&config);
    assert_eq!(
        context.local_package("https://registry.example:443/base/npm/widget").as_deref(),
        Some("widget"),
    );
    let hook = RouteHook::new(
        Arc::new(context),
        Identity::Anonymous,
        Arc::new(Mutex::new(Footprint::default())),
        Arc::from(b"secret".as_slice()),
    )
    .with_local_fetch(Arc::new(|_, _| panic!("unsafe local dispatch")));
    for url in [
        "https://REGISTRY.example/base/npm/widget",
        "https://registry.example/base/npm/~corp/%2e%2e/~other/widget",
        r"https://registry.example/base/npm\~corp/widget",
        "https://registry.example/base/npm/~co\trp/widget",
    ] {
        assert!(
            hook.fetch_local(url, reqwest::header::HeaderMap::new())
                .is_none(),
            "{url}",
        );
    }
}
