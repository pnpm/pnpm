use super::{
    HashMap, JSR_PACKAGE_BODY, LatestQuery, LockfileResolution, PACKAGE_BODY, ResolveOptions,
    UpdateBehavior, WantedDependency, assert_eq, build_resolver, build_resolver_with_registries,
};
use chrono::TimeZone;
use pnpm_resolving_resolver_base::Resolver;

#[tokio::test]
async fn range_specifier_picks_max_in_range() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    let name_ver = result.name_ver.as_ref().expect("npm resolver fills name_ver");
    assert_eq!(name_ver.name.to_string(), "acme");
    assert_eq!(name_ver.suffix.to_string(), "1.1.0");
    assert_eq!(result.id.as_str(), "acme@1.1.0");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.alias.as_deref(), Some("acme"));
    assert!(result.policy_violation.is_none());
    assert!(matches!(result.resolution, LockfileResolution::Tarball(_)));
}

#[tokio::test]
async fn missing_bare_specifier_synthesizes_default_tag_query() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
}

#[tokio::test]
async fn resolve_latest_returns_picked_manifest() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let query = LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some("acme".to_string()),
            bare_specifier: Some("^1.0.0".to_string()),
            ..WantedDependency::default()
        },
        compatible: false,
    };
    let info = resolver
        .resolve_latest(&query, &ResolveOptions::default())
        .await
        .unwrap()
        .expect("latest info");
    let manifest = info.latest_manifest.expect("manifest present");
    assert_eq!(manifest["version"].as_str(), Some("1.1.0"));
}

#[tokio::test]
async fn resolve_latest_under_compatible_does_not_override_update_to_latest() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let query = LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some("acme".to_string()),
            bare_specifier: Some("^1.0.0".to_string()),
            ..WantedDependency::default()
        },
        compatible: true,
    };
    let opts = ResolveOptions { update: UpdateBehavior::Off, ..ResolveOptions::default() };
    let info = resolver.resolve_latest(&query, &opts).await.unwrap().expect("latest info");
    let manifest = info.latest_manifest.expect("manifest present");
    assert_eq!(manifest["version"].as_str(), Some("1.1.0"));
}

#[tokio::test]
async fn jsr_specifier_without_selector_uses_default_tag() {
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
        bare_specifier: Some("jsr:@foo/bar".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(
        result.name_ver.as_ref().expect("npm resolver fills name_ver").suffix.to_string(),
        "1.1.0",
    );
    assert_eq!(result.resolved_via, "jsr-registry");
}

#[tokio::test]
async fn revision_refresh_revalidates_a_warm_packument_without_update_checksums() {
    let mut server = mockito::Server::new_async().await;
    let first_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };

    resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap();
    first_mock.assert_async().await;
    first_mock.remove_async().await;

    let refresh_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let opts = ResolveOptions {
        update: UpdateBehavior::Patches,
        update_checksums: false,
        ..ResolveOptions::default()
    };
    resolver.resolve(&wanted, &opts).await.unwrap();

    refresh_mock.assert_async().await;
}

#[tokio::test]
async fn latest_is_suppressed_when_all_versions_are_immature_fallback_case() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // Cutoff 2023-12-01 is before both versions → the pick falls back to the
    // lowest version; latest stays suppressed because the raw tag is immature.
    let published_by = Some(chrono::Utc.with_ymd_and_hms(2023, 12, 1, 0, 0, 0).unwrap());
    let opts = ResolveOptions { published_by, ..ResolveOptions::default() };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
    assert!(result.latest.is_none(), "immature dist-tags.latest suppresses the hint");
}
