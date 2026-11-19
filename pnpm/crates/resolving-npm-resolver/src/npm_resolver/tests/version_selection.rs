use super::{
    HashMap, JSR_PACKAGE_BODY, LatestQuery, LockfileResolution, PACKAGE_BODY, ResolveOptions,
    UpdateBehavior, WantedDependency, assert_eq, build_resolver, build_resolver_with_registries,
};
use pnpm_resolving_resolver_base::Resolver;

#[tokio::test]
async fn range_specifier_picks_max_in_range() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
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
    let name_ver = result.package.name_ver.as_ref().expect("npm resolver fills name_ver");
    assert_eq!(name_ver.name.to_string(), "acme");
    assert_eq!(name_ver.suffix.to_string(), "1.1.0");
    assert_eq!(result.id.as_str(), "acme@1.1.0");
    assert_eq!(result.package.latest.as_deref(), Some("1.1.0"));
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.alias.as_deref(), Some("acme"));
    assert!(result.policy_violation.is_none());
    assert!(matches!(result.resolution, LockfileResolution::Tarball(_)));
}

#[tokio::test]
async fn missing_bare_specifier_synthesizes_default_tag_query() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted =
        WantedDependency { alias: Some("acme".to_string()), ..WantedDependency::default() };
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

#[tokio::test]
async fn resolve_latest_returns_picked_manifest() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let query = LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some("acme".to_string()),
            bare_specifier: Some("^1.0.0".to_string()),
            ..WantedDependency::default()
        },
        compatible: false,
        current_version: None,
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
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let query = LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some("acme".to_string()),
            bare_specifier: Some("^1.0.0".to_string()),
            ..WantedDependency::default()
        },
        compatible: true,
        current_version: None,
    };
    let opts = ResolveOptions {
        refresh: pnpm_resolving_resolver_base::ResolutionRefreshOptions {
            update: UpdateBehavior::Off,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    let info = resolver
        .resolve_latest(&query, &opts)
        .await
        .unwrap()
        .expect("latest info");
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
    let result = resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        result.package.name_ver
            .as_ref()
            .expect("npm resolver fills name_ver")
            .suffix
            .to_string(),
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

    resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .unwrap();
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
        refresh: pnpm_resolving_resolver_base::ResolutionRefreshOptions {
            update: UpdateBehavior::Patches,
            update_checksums: false,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    resolver.resolve(&wanted, &opts).await.unwrap();

    refresh_mock.assert_async().await;
}

#[tokio::test]
async fn latest_is_suppressed_when_all_versions_are_immature_fallback_case() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    // Cutoff 2023-12-01 is before both versions → the pick falls back to the
    // lowest version; latest stays suppressed because the raw tag is immature.
    let published_by = Some("2023-12-01T00:00:00Z".parse::<jiff::Timestamp>().unwrap());
    let opts = ResolveOptions {
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            published_by,
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
        "1.0.0",
    );
    assert!(result.package.latest.is_none(), "immature dist-tags.latest suppresses the hint");
}

fn prerelease_channel_body(dist_tags: &[(&str, &str)]) -> String {
    let versions = [
        "1.0.0",
        "2.0.0-canary-2",
        "2.0.0-canary-abc123-20260101",
        "2.0.0-canary-def456-20260102",
        "2.0.0-next-1",
        "2.0.0-next-2",
        "2.0.0-rc.0",
        "2.0.0-rc.1",
        "2.0.0-rc.2",
        "2.0.0",
    ];
    let versions = versions
        .iter()
        .map(|version| {
            let manifest = serde_json::json!({
                "name": "acme",
                "version": version,
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "tarball": format!("https://registry/acme-{version}.tgz"),
                },
            });
            ((*version).to_string(), manifest)
        })
        .collect::<serde_json::Map<_, _>>();
    let dist_tags = dist_tags
        .iter()
        .map(|(tag, version)| ((*tag).to_string(), serde_json::Value::from(*version)))
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({ "name": "acme", "dist-tags": dist_tags, "versions": versions }).to_string()
}

struct PrereleaseChannelCase {
    dist_tags: &'static [(&'static str, &'static str)],
    current: &'static str,
    expected: &'static str,
}

#[tokio::test]
async fn resolve_latest_follows_the_prerelease_channel_of_the_current_version() {
    let cases = [
        PrereleaseChannelCase {
            dist_tags: &[("latest", "1.0.0"), ("next", "2.0.0-next-2")],
            current: "2.0.0-next-1",
            expected: "2.0.0-next-2",
        },
        PrereleaseChannelCase {
            dist_tags: &[("latest", "1.0.0"), ("beta", "2.0.0-rc.0"), ("next", "2.0.0-rc.2")],
            current: "2.0.0-rc.1",
            expected: "2.0.0-rc.2",
        },
        PrereleaseChannelCase {
            dist_tags: &[("latest", "2.0.0"), ("next", "2.0.0-next-2")],
            current: "2.0.0-next-1",
            expected: "2.0.0",
        },
        PrereleaseChannelCase {
            dist_tags: &[("latest", "1.0.0"), ("canary", "2.0.0-canary-def456-20260102")],
            current: "2.0.0-canary-abc123-20260101",
            expected: "2.0.0-canary-def456-20260102",
        },
        PrereleaseChannelCase {
            dist_tags: &[
                ("latest", "1.0.0"),
                ("next", "2.0.0-next-2"),
                ("preview", "2.0.0-next-9"),
            ],
            current: "2.0.0-next-1",
            expected: "2.0.0-next-2",
        },
        PrereleaseChannelCase {
            dist_tags: &[("latest", "1.0.0"), ("canary", "2.0.0-canary-2")],
            current: "2.0.0-next-1",
            expected: "2.0.0-next-1",
        },
    ];
    for PrereleaseChannelCase { dist_tags, current, expected } in cases {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/acme")
            .with_status(200)
            .with_body(prerelease_channel_body(dist_tags))
            .create_async()
            .await;
        let registry = format!("{}/", server.url());
        let (resolver, _tempdir) = build_resolver(&registry);

        let query = LatestQuery {
            wanted_dependency: WantedDependency {
                alias: Some("acme".to_string()),
                bare_specifier: Some(current.to_string()),
                ..WantedDependency::default()
            },
            compatible: false,
            current_version: Some(current.parse().expect("valid version")),
        };
        let info = resolver
            .resolve_latest(&query, &ResolveOptions::default())
            .await
            .unwrap()
            .expect("latest info");
        let manifest = info.latest_manifest.expect("manifest present");
        assert_eq!(manifest["version"].as_str(), Some(expected), "current {current}");
    }
}

#[tokio::test]
async fn resolve_latest_ignores_an_immature_prerelease_channel_tag() {
    let manifest = |version: &str| {
        serde_json::json!({
            "name": "acme",
            "version": version,
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "tarball": format!("https://registry/acme-{version}.tgz"),
            },
        })
    };
    let body = serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "2.0.0", "next": "3.0.0-beta.0" },
        "modified": "2026-03-01T00:00:00.000Z",
        "time": {
            "2.0.0-beta.0": "2026-01-01T00:00:00.000Z",
            "2.0.0": "2026-01-02T00:00:00.000Z",
            "3.0.0-beta.0": "2026-03-01T00:00:00.000Z",
        },
        "versions": {
            "2.0.0-beta.0": manifest("2.0.0-beta.0"),
            "2.0.0": manifest("2.0.0"),
            "3.0.0-beta.0": manifest("3.0.0-beta.0"),
        },
    })
    .to_string();
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(body)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let query = LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some("acme".to_string()),
            bare_specifier: Some("2.0.0-beta.0".to_string()),
            ..WantedDependency::default()
        },
        compatible: false,
        current_version: Some("2.0.0-beta.0".parse().expect("valid version")),
    };
    let opts = ResolveOptions {
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            published_by: Some("2026-02-01T00:00:00Z".parse::<jiff::Timestamp>().unwrap()),
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    let info = resolver
        .resolve_latest(&query, &opts)
        .await
        .unwrap()
        .expect("latest info");
    let manifest = info.latest_manifest.expect("manifest present");
    assert_eq!(manifest["version"].as_str(), Some("2.0.0"));
}
