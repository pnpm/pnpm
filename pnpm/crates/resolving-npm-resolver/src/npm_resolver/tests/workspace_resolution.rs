use super::{
    LockfileResolution, ResolveFromWorkspaceError, ResolveOptions, RetryOpts, UpdateBehavior,
    WantedDependency, assert_eq, build_resolver, build_workspace_packages,
    build_workspace_packages_at, is_not_found_error, single_version_body,
    workspace_resolve_options,
};
use pnpm_resolving_resolver_base::Resolver;

#[tokio::test]
async fn workspace_path_form_falls_through_to_local_resolver() {
    let server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("workspace:./acme".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap();
    assert!(result.is_none());
}

/// The case behind [#11929] (babylon's `@dev/build-tools` isn't on
/// npm, so bare-semver must resolve via the workspace).
///
/// [#11929]: https://github.com/pnpm/pnpm/issues/11929
#[tokio::test]
async fn falls_back_to_workspace_when_registry_returns_404() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
    match &result.resolution {
        LockfileResolution::Directory(dir) => assert_eq!(dir.directory, "../acme"),
        other => panic!("expected directory resolution, got {other:?}"),
    }
}

#[tokio::test]
async fn revision_qualified_selector_does_not_fall_back_to_workspace() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0+r1".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &opts)
        .await
        .expect_err("a registry revision cannot resolve to an unversioned workspace artifact");
    assert!(is_not_found_error(err.as_ref()), "expected registry 404, got: {err}");
}

#[tokio::test]
async fn revision_refresh_preserves_an_implicit_workspace_resolution() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let mut opts = workspace_resolve_options(packages);
    opts.update = UpdateBehavior::Patches;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
}

#[tokio::test]
async fn workspace_shadows_registry_when_name_and_version_match() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.0.0",
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace shadow");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
    // `latest` is back-stamped from the registry packument so the
    // install layer can still surface upgrade hints.
    assert_eq!(result.latest.as_deref(), Some("1.0.0"));
}

#[tokio::test]
async fn registry_version_higher_than_workspace_keeps_registry_pick() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.1.0",
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("registry pick");
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.id.as_str(), "acme@1.1.0");
}

#[tokio::test]
async fn workspace_higher_version_shadows_registry_pick() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.0.0",
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["2.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some(">=1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace shadow");
    assert_eq!(result.resolved_via, "workspace");
}

#[tokio::test]
async fn injected_workspace_match_emits_file_resolution() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.0.0",
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        injected: Some(true),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace shadow");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "file:packages/acme");
    match &result.resolution {
        LockfileResolution::Directory(dir) => assert_eq!(dir.directory, "packages/acme"),
        other => panic!("expected directory resolution, got {other:?}"),
    }
}

#[tokio::test]
async fn workspace_fallback_picks_highest_version_for_latest_tag() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages_at(
        "acme",
        &[
            ("1.0.0", "/repo/packages/acme-1.0.0"),
            ("1.1.0", "/repo/packages/acme-1.1.0"),
            ("2.0.0", "/repo/packages/acme-2.0.0"),
        ],
    );
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("latest".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme-2.0.0");
}

#[tokio::test]
async fn workspace_fallback_resolves_specific_version_request() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages_at(
        "acme",
        &[
            ("1.0.0", "/repo/packages/acme-1.0.0"),
            ("1.1.0", "/repo/packages/acme-1.1.0"),
            ("2.0.0", "/repo/packages/acme-2.0.0"),
        ],
    );
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.1.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme-1.1.0");
}

/// Covers the `Ok(None)` fallback arm (200 + no matching version),
/// distinct from the `Err` 404 arm.
#[tokio::test]
async fn workspace_fallback_kicks_in_when_registry_lacks_requested_version() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "1.0.0",
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["100.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("100.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
}

#[tokio::test]
async fn workspace_version_mismatch_surfaces_for_exact_request_on_registry_404() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &opts)
        .await
        .expect_err("workspace can't satisfy 2.0.0; workspace version mismatch must surface");
    assert!(
        err.downcast_ref::<ResolveFromWorkspaceError>().is_some_and(|ws_err| matches!(
            ws_err,
            ResolveFromWorkspaceError::NoMatchingVersionInsideWorkspace { .. }
        )),
        "expected NoMatchingVersionInsideWorkspace, got: {err}",
    );
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("No matching version found for acme@2.0.0 inside the workspace"),
        "expected the workspace mismatch message, got: {err_msg}",
    );
    assert!(
        err_msg.contains("Available versions: 1.0.0"),
        "expected error to list available workspace versions, got: {err_msg}",
    );
}

#[tokio::test]
async fn registry_pick_wins_when_workspace_version_does_not_match() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "3.1.0",
            "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("3.1.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("registry pick");
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.id.as_str(), "acme@3.1.0");
}

#[tokio::test]
async fn workspace_version_mismatch_surfaces_for_range_request_on_registry_404() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &opts)
        .await
        .expect_err("registry 404 and no matching workspace version must fail");
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("No matching version found for acme@^2.0.0 inside the workspace"),
        "expected the workspace mismatch message, got: {err_msg}",
    );
    assert!(
        err_msg.contains("Available versions: 1.0.0"),
        "expected error to list available workspace versions, got: {err_msg}",
    );
}

#[tokio::test]
async fn workspace_version_mismatch_surfaces_when_registry_lacks_matching_version() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(single_version_body(
            "2.0.0",
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        ))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^3.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &opts)
        .await
        .expect_err("neither registry nor workspace satisfies ^3.0.0");
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("No matching version found for acme@^3.0.0 inside the workspace"),
        "expected the workspace mismatch message, got: {err_msg}",
    );
    assert!(
        err_msg.contains("Available versions: 1.0.0"),
        "expected error to list available workspace versions, got: {err_msg}",
    );
}

#[tokio::test]
async fn workspace_fallback_succeeds_for_range_request_on_registry_404() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
}

#[tokio::test]
async fn non_404_registry_error_not_masked_by_workspace_version_mismatch() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(500).create_async().await;
    let registry = format!("{}/", server.url());
    let (mut resolver, _tempdir) = build_resolver(&registry);
    // A 5xx is retried with backoff; skip the retries so the test
    // doesn't spend over a minute sleeping.
    resolver.retry_opts = RetryOpts { retries: 0, ..RetryOpts::default() };

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &opts)
        .await
        .expect_err("a 500 registry response must propagate as an error");
    let err_msg = err.to_string();
    assert!(err_msg.contains("500"), "expected the 500 to propagate, got: {err_msg}");
    assert!(
        !err_msg.contains("inside the workspace"),
        "workspace mismatch must not mask a non-404 registry error, got: {err_msg}",
    );
}
