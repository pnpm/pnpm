use super::{
    MINIMUM_RELEASE_AGE_VIOLATION_CODE, PACKAGE_BODY, ResolveOptions, WantedDependency, assert_eq,
    build_resolver, create_package_version_policy,
};
use chrono::TimeZone;
use pnpm_resolving_resolver_base::Resolver;

#[tokio::test]
async fn surfaces_min_release_age_violation_inline() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // Cutoff sits between 1.0.0 (2024-01-10) and 1.1.0 (2024-12-10):
    // the picker should fall back to 1.0.0 as the highest mature
    // version and the picked result should *not* trip a violation.
    // To force a violation we set the cutoff before both versions.
    let published_by = Some(chrono::Utc.with_ymd_and_hms(2023, 12, 1, 0, 0, 0).unwrap());
    let opts = ResolveOptions { published_by, ..ResolveOptions::default() };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    let violation = result.policy_violation.expect("violation surfaced");
    assert_eq!(violation.code, MINIMUM_RELEASE_AGE_VIOLATION_CODE);
}

#[tokio::test]
async fn latest_is_suppressed_when_published_by_holds_back_raw_latest() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // PACKAGE_BODY has 1.0.0 (2024-01-10) and 1.1.0 (2024-12-10),
    // dist-tags.latest = 1.1.0. Cutoff 2024-06-01 leaves 1.1.0 immature:
    // the hint must not fire rather than name a non-latest version.
    let published_by = Some(chrono::Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap());
    let opts = ResolveOptions { published_by, ..ResolveOptions::default() };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
    assert!(result.latest.is_none(), "immature dist-tags.latest suppresses the hint");
    assert!(result.policy_violation.is_none(), "1.0.0 is mature, no violation");
}

#[tokio::test]
async fn latest_is_raw_registry_tag_when_it_satisfies_published_by() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // Cutoff 2025-01-01 is after both versions, so the pinned 1.0.0 install
    // still advertises the mature 1.1.0.
    let published_by = Some(chrono::Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap());
    let opts = ResolveOptions { published_by, ..ResolveOptions::default() };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
}

#[tokio::test]
async fn latest_is_raw_registry_tag_when_published_by_is_none() {
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
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
}

#[tokio::test]
async fn latest_is_raw_registry_tag_when_published_by_exclude_matches_package() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // The exclude policy disables the maturity policy for `acme` entirely, so
    // neither the pick nor the latest hint may be affected by the cutoff.
    let published_by = Some(chrono::Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap());
    let exclude = create_package_version_policy(["acme"]).expect("policy");
    let opts = ResolveOptions {
        published_by,
        published_by_exclude: Some(exclude),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
    assert!(result.policy_violation.is_none(), "excluded package has no violation");
}

#[tokio::test]
async fn latest_is_raw_registry_tag_when_published_by_exclude_trusts_that_version() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let published_by = Some(chrono::Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap());
    let exclude = create_package_version_policy(["acme@1.1.0"]).expect("policy");
    let opts = ResolveOptions {
        published_by,
        published_by_exclude: Some(exclude),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
}
