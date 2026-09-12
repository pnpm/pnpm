use super::{
    GuardExhaustionPolicy, MISMATCHED_KEY_BODY, PACKAGE_BODY, ResolveOptions, WantedDependency,
    assert_eq, build_resolver, guard_rejecting, packument_with_many_versions, reject_versions,
};
use pnpm_resolving_resolver_base::Resolver;

#[tokio::test]
async fn package_version_guard_excludes_rejected_versions_and_repicks() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let opts = ResolveOptions {
        package_version_guard: Some(reject_versions(&["1.1.0"])),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };

    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    let name_ver = result.name_ver.as_ref().expect("name_ver");
    assert_eq!(name_ver.suffix.to_string(), "1.0.0");
    assert_eq!(result.latest.as_deref(), Some("1.0.0"));
}

#[tokio::test]
async fn package_version_guard_repopulates_latest_tag() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let opts = ResolveOptions {
        package_version_guard: Some(reject_versions(&["1.1.0"])),
        ..ResolveOptions::default()
    };
    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };

    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
    assert_eq!(result.latest.as_deref(), Some("1.0.0"));
}

#[tokio::test]
async fn package_version_guard_blocking_every_version_errors() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let opts = ResolveOptions {
        package_version_guard: Some(reject_versions(&["1.0.0", "1.1.0"])),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };

    // Every matching version is rejected, so the resolver must surface a
    // clear guard error rather than Ok(None) (which would read as an
    // unsupported spec downstream).
    let err = resolver.resolve(&wanted, &opts).await.expect_err("expected a guard error");
    let message = err.to_string();
    assert!(message.contains("acme"), "{message}");
    assert!(message.contains("rejected by the resolver guard"), "{message}");
}

#[tokio::test]
async fn package_version_guard_accepting_rejected_falls_back_to_the_unguarded_pick() {
    let mut server = mockito::Server::new_async().await;
    let _mock =
        server.mock("GET", "/acme").with_status(200).with_body(PACKAGE_BODY).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let opts = ResolveOptions {
        package_version_guard: Some(guard_rejecting(
            &["1.0.0", "1.1.0"],
            GuardExhaustionPolicy::AcceptRejected,
        )),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };

    // The guard states a preference, so the request resolves to the version
    // it would have picked with no guard at all.
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.1.0");
}

#[tokio::test]
async fn package_version_guard_accepting_rejected_falls_back_at_the_repick_limit() {
    const VERSION_COUNT: u32 = 1005;
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(packument_with_many_versions(VERSION_COUNT))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let blocked: Vec<String> = (0..VERSION_COUNT).map(|patch| format!("1.0.{patch}")).collect();
    let opts = ResolveOptions {
        package_version_guard: Some(guard_rejecting(
            &blocked.iter().map(String::as_str).collect::<Vec<_>>(),
            GuardExhaustionPolicy::AcceptRejected,
        )),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };

    // The re-pick cap stops the search well before the candidates run out.
    // A guard whose rejections are a preference must still get an answer
    // there, not lose the whole resolve to the cap.
    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(
        result.name_ver.as_ref().expect("name_ver").suffix.to_string(),
        format!("1.0.{}", VERSION_COUNT - 1),
    );
}

#[tokio::test]
async fn package_version_guard_blocks_the_packument_key_not_the_parsed_version() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(MISMATCHED_KEY_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // The guard rejects the parsed manifest version `1.5.0`, whose
    // packument key is `1.5.0+build`. The repick must still exclude that
    // entry and fall back to `1.0.0`, rather than wrongly reporting that
    // every version is blocked.
    let opts = ResolveOptions {
        package_version_guard: Some(reject_versions(&["1.5.0"])),
        ..ResolveOptions::default()
    };
    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };

    let result = resolver.resolve(&wanted, &opts).await.unwrap().unwrap();
    assert_eq!(result.name_ver.as_ref().expect("name_ver").suffix.to_string(), "1.0.0");
}
