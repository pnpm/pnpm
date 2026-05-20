use std::{collections::HashMap, sync::Arc};

use chrono::TimeZone;
use pacquet_lockfile::LockfileResolution;
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveOptions, Resolver, UpdateBehavior, WantedDependency,
};
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::{
    npm_resolver::NpmResolver, pick_package::InMemoryPackageMetaCache,
    violation_codes::MINIMUM_RELEASE_AGE_VIOLATION_CODE,
};

const PACKAGE_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.1.0" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "acme",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/acme-1.0.0.tgz"
            }
        },
        "1.1.0": {
            "name": "acme",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/acme-1.1.0.tgz"
            }
        }
    }
}"#;

fn build_resolver(registry: &str) -> (NpmResolver<InMemoryPackageMetaCache>, TempDir) {
    let cache_dir = TempDir::new().expect("tempdir");
    let mut registries = HashMap::new();
    registries.insert("default".to_string(), registry.to_string());
    let resolver = NpmResolver {
        registries,
        named_registries: HashMap::new(),
        http_client: Arc::new(ThrottledClient::default()),
        auth_headers: Arc::new(AuthHeaders::default()),
        meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
        cache_dir: Some(cache_dir.path().to_path_buf()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
    };
    (resolver, cache_dir)
}

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
    assert_eq!(result.id.name.to_string(), "acme");
    assert_eq!(result.id.suffix.to_string(), "1.1.0");
    assert_eq!(result.latest.as_deref(), Some("1.1.0"));
    assert_eq!(result.resolved_via, "npm-registry");
    assert_eq!(result.alias.as_deref(), Some("acme"));
    assert!(result.policy_violation.is_none());
    assert!(matches!(result.resolution, LockfileResolution::Tarball(_)));
}

#[tokio::test]
async fn workspace_specifier_returns_none_for_chain_fallthrough() {
    let server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let (resolver, _tempdir) = build_resolver(&registry);

    let wanted = WantedDependency {
        alias: Some("acme".to_string()),
        bare_specifier: Some("workspace:*".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap();
    assert!(result.is_none());
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
    assert_eq!(result.id.suffix.to_string(), "1.1.0");
}

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
