use super::{PACKAGE_BODY, ResolveOptions, WantedDependency, assert_eq, build_resolver};
use pnpm_resolving_resolver_base::Resolver;

#[tokio::test]
async fn empty_specifier_resolves_to_the_max_published_version() {
    // Regression test for pnpm/pnpm#13673.
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some(String::new()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(result.id.as_str(), "acme@1.1.0");
    assert_eq!(result.resolved_via, "npm-registry");
}

/// Regression test for pnpm/pnpm#14096: npm strips build metadata when
/// it publishes a version, so a selector carrying it must still resolve
/// to the published version — for a prerelease as much as for a stable
/// release.
#[tokio::test]
async fn exact_version_with_build_metadata_resolves_to_the_published_version() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    for (bare_specifier, expected_id) in
        [("1.0.0+build1", "acme@1.0.0"), ("1.0.0-canary.1+build1", "acme@1.0.0-canary.1")]
    {
        let wanted = WantedDependency {
            alias: Some("acme".to_string()),
            bare_specifier: Some(bare_specifier.to_string()),
            ..WantedDependency::default()
        };
        let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
        assert_eq!(result.id.as_str(), expected_id, "for {bare_specifier:?}");
        assert_eq!(result.resolved_via, "npm-registry", "for {bare_specifier:?}");
    }
}
