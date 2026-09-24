use super::{
    ResolveOptions, TRUST_DOWNGRADE_PACKAGE_BODY, TrustPolicy, WantedDependency, assert_eq,
    build_resolver, trust_downgrade_body_without_time,
};
use chrono::TimeZone;
use pnpm_resolving_resolver_base::Resolver;

async fn resolve_under_no_downgrade(
    body: &str,
    bare_specifier: &str,
    published_by: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<
    Option<pnpm_resolving_resolver_base::ResolveResult>,
    pnpm_resolving_resolver_base::ResolveError,
> {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(body)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let opts = ResolveOptions {
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            trust_policy: Some(TrustPolicy::NoDowngrade),
            published_by,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some(bare_specifier.to_string()),
        ..WantedDependency::default()
    };
    resolver.resolve(&wanted, &opts).await
}

fn resolved_version(result: &pnpm_resolving_resolver_base::ResolveResult) -> String {
    result.package.name_ver
        .as_ref()
        .expect("name_ver")
        .suffix
        .to_string()
}

#[tokio::test]
async fn trust_downgrade_at_resolve_time_falls_back_to_the_previous_version() {
    for bare_specifier in ["^1.0.0", "latest"] {
        let result = resolve_under_no_downgrade(TRUST_DOWNGRADE_PACKAGE_BODY, bare_specifier, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resolved_version(&result), "1.0.0", "spec {bare_specifier}");
        assert_eq!(result.package.latest.as_deref(), Some("1.1.0"), "spec {bare_specifier}");
    }
}

#[tokio::test]
async fn trust_downgrade_fallback_also_skips_versions_younger_than_minimum_release_age() {
    let mut body: serde_json::Value =
        serde_json::from_str(TRUST_DOWNGRADE_PACKAGE_BODY).expect("parse fixture packument");
    let mut trusted_release = body["versions"]["1.0.0"].clone();
    trusted_release["version"] = "1.2.0".into();
    body["versions"]["1.2.0"] = trusted_release;
    body["time"]["1.2.0"] = "2025-01-10T08:30:00.000Z".into();
    body["dist-tags"]["latest"] = "1.2.0".into();
    let published_by = Some(chrono::Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap());

    let result = resolve_under_no_downgrade(&body.to_string(), "^1.0.0", published_by)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved_version(&result), "1.0.0");
}

#[tokio::test]
async fn trust_downgrade_at_resolve_time_fails_when_no_other_version_satisfies_the_spec() {
    let err = resolve_under_no_downgrade(TRUST_DOWNGRADE_PACKAGE_BODY, "1.1.0", None)
        .await
        .expect_err("trust downgrade should fail");
    assert!(err.to_string().contains("trust downgrade"), "got {err}");
}

#[tokio::test]
async fn trust_check_fails_at_resolve_time_when_the_registry_serves_no_time_field() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(trust_downgrade_body_without_time())
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let opts = ResolveOptions {
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            trust_policy: Some(TrustPolicy::NoDowngrade),
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver.resolve(&wanted, &opts).await.expect_err("missing time should fail closed");
    assert!(err.to_string().contains(r#"missing the "time" field"#), "got {err}");
}

#[tokio::test]
async fn trust_check_skipped_at_resolve_time_when_missing_time_is_ignored() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(trust_downgrade_body_without_time())
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (mut resolver, _tempdir) = build_resolver(&registry);
    resolver.cache_policy.ignore_missing_time_field = true;

    let opts = ResolveOptions {
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            trust_policy: Some(TrustPolicy::NoDowngrade),
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver
        .resolve(&wanted, &opts)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        result.package.name_ver
            .as_ref()
            .expect("name_ver")
            .suffix
            .to_string(),
        "1.1.0",
    );
}

#[tokio::test]
async fn trust_downgrade_ignored_when_trust_policy_off() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(TRUST_DOWNGRADE_PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        result.package.name_ver
            .as_ref()
            .expect("name_ver")
            .suffix
            .to_string(),
        "1.1.0",
    );
}
