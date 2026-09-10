use super::{
    HashMap, JSR_PACKAGE_BODY, ResolveOptions, WantedDependency, assert_eq,
    build_resolver_with_registries,
};
use chrono::TimeZone;
use pnpm_resolving_resolver_base::Resolver;

#[tokio::test]
async fn jsr_specifier_suppresses_latest_when_published_by_holds_back_raw_latest() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/@jsr%2Ffoo__bar")
        .with_status(200)
        .with_body(JSR_PACKAGE_BODY)
        .create_async()
        .await;
    let jsr_registry = format!("{}/", server.url());
    let mut registries = HashMap::new();
    registries.insert("default".to_string(), "https://registry.npmjs.org/".to_string());
    registries.insert("@jsr".to_string(), jsr_registry);
    let (resolver, _tempdir) = build_resolver_with_registries(registries);

    let wanted = WantedDependency {
        alias: Some("@foo/bar".to_string()),
        bare_specifier: Some("jsr:@foo/bar@^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions {
        published_by: Some(chrono::Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap()),
        ..ResolveOptions::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
    assert!(result.latest.is_none(), "immature dist-tags.latest suppresses the hint");
}
