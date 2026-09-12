use super::{
    CurrentPkg, LockfileResolution, PkgResolutionId, RegistryResolution, ResolveOptions,
    TrustPolicy, UpdateBehavior, WantedDependency, assert_eq, build_resolver,
    build_workspace_packages, build_workspace_packages_at, single_version_body,
    workspace_resolve_options,
};
use pnpm_resolving_resolver_base::Resolver;

#[tokio::test]
async fn workspace_version_without_workspace_packages_surfaces_error() {
    let server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("workspace:*".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .expect_err("workspace_packages must be populated for workspace: specifiers");
    let message = err.to_string();
    assert!(
        message.contains("workspace packages were not loaded"),
        "unexpected error message: {message}",
    );
}

#[tokio::test]
async fn revision_refresh_does_not_replace_a_registry_resolution_with_a_workspace_package() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
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
    let mut opts = workspace_resolve_options(packages);
    opts.update = UpdateBehavior::Patches;
    opts.current_pkg = Some(CurrentPkg {
        id: PkgResolutionId::from("acme@1.0.0"),
        name: Some("acme".to_string()),
        version: Some("1.0.0".to_string()),
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
                .parse()
                .unwrap(),
            revision: None,
        }),
        published_at: None,
    });

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("registry pick");
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.id.as_str(), "acme@1.0.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn link_workspace_packages_off_skips_workspace_match() {
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
    let mut opts = workspace_resolve_options(packages);
    opts.link_workspace_packages = pnpm_config::LinkWorkspacePackages::Off;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("registry pick");
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.id.as_str(), "acme@1.0.0");
}

#[tokio::test]
async fn prefer_workspace_packages_keeps_workspace_over_newer_registry() {
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
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
}

#[tokio::test]
async fn prefer_workspace_packages_skips_the_registry_entirely() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").expect(0).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["1.0.0"]);
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
    assert_eq!(result.latest, None);
    mock.assert_async().await;
}

#[tokio::test]
async fn prefer_workspace_packages_still_consults_registry_for_several_local_copies() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
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

    let packages = build_workspace_packages_at(
        "acme",
        &[("1.0.0", "/repo/packages/acme-1"), ("1.1.0", "/repo/packages/acme-11")],
    );
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    mock.assert_async().await;
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme-11");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
}

#[tokio::test]
async fn prefer_workspace_packages_still_consults_registry_for_injected_deps() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
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
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        injected: Some(true),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    mock.assert_async().await;
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
}

#[tokio::test]
async fn prefer_workspace_packages_does_not_engage_without_a_matching_local_version() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
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
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^2.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("registry pick");
    mock.assert_async().await;
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.id.as_str(), "acme@2.0.0");
}

#[tokio::test]
async fn prefer_workspace_packages_still_consults_registry_under_no_downgrade() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
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
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
    mock.assert_async().await;
}

#[tokio::test]
async fn prefer_workspace_packages_still_consults_registry_when_updating_checksums() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
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
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;
    opts.update_checksums = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
    mock.assert_async().await;
}

#[tokio::test]
async fn prefer_workspace_packages_still_consults_registry_when_injecting_workspace_packages() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
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
    let mut opts = workspace_resolve_options(packages);
    opts.prefer_workspace_packages = true;
    opts.inject_workspace_packages = true;

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace pick");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
    mock.assert_async().await;
}

/// Exercises the `includePrerelease` arm of `resolve_workspace_range`.
#[tokio::test]
async fn workspace_fallback_picks_local_prerelease_for_latest_tag() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("acme", &["3.0.0-alpha.1.2.3"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("latest".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &opts).await.unwrap().expect("workspace fallback");
    assert_eq!(result.resolved_via, "workspace");
    assert_eq!(result.id.as_str(), "link:../acme");
}

#[tokio::test]
async fn registry_404_propagates_when_package_not_in_workspace() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/acme").with_status(404).create_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let packages = build_workspace_packages("other-pkg", &["1.0.0"]);
    let opts = workspace_resolve_options(packages);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("^1.0.0".to_string()),
        ..WantedDependency::default()
    };
    let err = resolver
        .resolve(&wanted, &opts)
        .await
        .expect_err("package absent from both registry and workspace must fail");
    let err_msg = err.to_string();
    assert!(err_msg.contains("404"), "expected the 404 to propagate, got: {err_msg}");
    assert!(
        !err_msg.contains("inside the workspace"),
        "workspace mismatch must not surface when the package is not in the workspace, got: {err_msg}",
    );
}
