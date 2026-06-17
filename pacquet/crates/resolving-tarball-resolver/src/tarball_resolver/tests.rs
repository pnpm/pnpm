use std::sync::Arc;

use pacquet_lockfile::LockfileResolution;
use pacquet_network::ThrottledClient;
use pacquet_resolving_resolver_base::{LatestQuery, ResolveOptions, Resolver, WantedDependency};
use pretty_assertions::assert_eq;

use crate::TarballResolver;

fn build_resolver() -> TarballResolver {
    TarballResolver { http_client: Arc::new(ThrottledClient::default()), fetch_context: None }
}

fn tarball_url(resolution: &LockfileResolution) -> &str {
    match resolution {
        LockfileResolution::Tarball(t) => t.tarball.as_str(),
        other => panic!("expected Tarball resolution, got {other:?}"),
    }
}

#[tokio::test]
async fn non_http_bare_specifier_returns_none_so_the_chain_falls_through() {
    let resolver = build_resolver();
    let wanted = WantedDependency {
        alias: Some("foo".to_string()),
        bare_specifier: Some("git+ssh://git@github.com/foo/bar.git".to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn missing_bare_specifier_returns_none() {
    let resolver = build_resolver();
    let wanted = WantedDependency { alias: Some("foo".to_string()), ..WantedDependency::default() };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn mutable_response_stores_normalized_request_url() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("HEAD", "/pkg-1.0.0.tgz").with_status(200).create_async().await;
    let url = format!("{}/pkg-1.0.0.tgz", server.url());

    let resolver = build_resolver();
    let wanted = WantedDependency {
        alias: Some("pkg".to_string()),
        bare_specifier: Some(url.clone()),
        ..WantedDependency::default()
    };
    let result =
        resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().expect("claim");

    assert_eq!(result.id.to_string(), url);
    assert_eq!(result.normalized_bare_specifier.as_deref(), Some(url.as_str()));
    assert_eq!(tarball_url(&result.resolution), url);
    assert_eq!(result.resolved_via, "url");
}

#[tokio::test]
async fn immutable_response_without_redirect_keeps_the_requested_url() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("HEAD", "/pkg-1.0.0.tgz")
        .with_status(200)
        .with_header("cache-control", "public, max-age=31536000, immutable")
        .create_async()
        .await;
    let url = format!("{}/pkg-1.0.0.tgz", server.url());

    let resolver = build_resolver();
    let wanted = WantedDependency {
        alias: Some("pkg".to_string()),
        bare_specifier: Some(url.clone()),
        ..WantedDependency::default()
    };
    let result =
        resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().expect("claim");

    assert_eq!(tarball_url(&result.resolution), url);
}

#[tokio::test]
async fn immutable_response_after_redirect_records_the_final_url() {
    let mut server = mockito::Server::new_async().await;
    let final_path = "/canonical-1.0.0.tgz";
    let final_url = format!("{}{}", server.url(), final_path);
    let _redirect = server
        .mock("HEAD", "/redirected-1.0.0.tgz")
        .with_status(301)
        .with_header("location", &final_url)
        .create_async()
        .await;
    let _final = server
        .mock("HEAD", final_path)
        .with_status(200)
        .with_header("cache-control", "immutable")
        .create_async()
        .await;
    let requested_url = format!("{}/redirected-1.0.0.tgz", server.url());

    let resolver = build_resolver();
    let wanted = WantedDependency {
        alias: Some("pkg".to_string()),
        bare_specifier: Some(requested_url.clone()),
        ..WantedDependency::default()
    };
    let result =
        resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().expect("claim");

    assert_eq!(result.id.to_string(), requested_url);
    assert_eq!(result.normalized_bare_specifier.as_deref(), Some(requested_url.as_str()));
    assert_eq!(tarball_url(&result.resolution), final_url);
}

#[tokio::test]
async fn resolve_latest_claims_http_specifiers() {
    let resolver = build_resolver();
    let query = LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some("pkg".to_string()),
            bare_specifier: Some("https://example.invalid/pkg.tgz".to_string()),
            ..WantedDependency::default()
        },
        compatible: false,
    };
    let info = resolver
        .resolve_latest(&query, &ResolveOptions::default())
        .await
        .unwrap()
        .expect("latest claim");
    assert!(info.latest_manifest.is_none());
}

#[tokio::test]
async fn resolve_latest_returns_none_for_non_http_specifiers() {
    let resolver = build_resolver();
    let query = LatestQuery {
        wanted_dependency: WantedDependency {
            alias: Some("pkg".to_string()),
            bare_specifier: Some("git+ssh://git@github.com/foo/bar.git".to_string()),
            ..WantedDependency::default()
        },
        compatible: false,
    };
    let info = resolver.resolve_latest(&query, &ResolveOptions::default()).await.unwrap();
    assert!(info.is_none());
}
