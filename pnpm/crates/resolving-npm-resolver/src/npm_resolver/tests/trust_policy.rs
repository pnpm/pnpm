use super::{
    ResolveOptions, TRUST_DOWNGRADE_PACKAGE_BODY, TrustPolicy, WantedDependency, assert_eq,
    build_resolver, trust_downgrade_body_without_time,
};
use pnpm_resolving_resolver_base::Resolver;

#[tokio::test]
async fn trust_downgrade_at_resolve_time_fails_under_no_downgrade() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(TRUST_DOWNGRADE_PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let opts = ResolveOptions {
        trust_policy: Some(TrustPolicy::NoDowngrade),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver.resolve(&wanted, &opts).await.expect_err("trust downgrade should fail");
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
        trust_policy: Some(TrustPolicy::NoDowngrade),
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
    resolver.ignore_missing_time_field = true;

    let opts = ResolveOptions {
        trust_policy: Some(TrustPolicy::NoDowngrade),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
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
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
}
