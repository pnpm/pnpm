mod artifact_requests;

mod behavior;

mod packument_rewriting;

mod circuit_breaker;

mod metadata_requests;

use super::{
    CacheValidators, CircuitBreaker, FetchOutcome, PackumentFetch, UPSTREAM_ERROR_BODY_LIMIT,
    Upstream, abbreviate_packument, extract_version_manifest, rewrite_tarball_urls,
    rewrite_upstream_tarball_urls, tarball_basename,
};
use chrono::{DateTime, TimeZone, Utc};
use pnpr_config::UpstreamConfig;
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde_json::json;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// Build an [`Upstream`] pointing at `url` with `headers`, all per-upstream
/// tuning knobs at their verdaccio defaults.
fn upstream(url: String, headers: HeaderMap) -> Upstream {
    Upstream::new("npmjs", &UpstreamConfig::with_defaults(url, headers))
}

/// Fixed "current time" for abbreviation tests so the `time`-map
/// coarsening (which buckets entries by age) is deterministic.
fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 3, 20, 12, 0, 0).unwrap()
}

/// Build a header map carrying a bearer `Authorization` plus one
/// custom header — the resolved per-upstream set an [`Upstream`] is
/// expected to attach to every request.
fn auth_and_custom_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret-token"));
    headers.insert("x-org", HeaderValue::from_static("acme"));
    headers
}

/// Build an [`Upstream`] pointing at `url` with a circuit breaker armed
/// at `max_fails` and a long cooldown, so a test can drive it to the
/// open state and observe the short-circuit before the cooldown lapses.
fn breaking_upstream(url: String, max_fails: u32) -> Upstream {
    Upstream::new(
        "npmjs",
        &UpstreamConfig {
            url,
            headers: HeaderMap::new(),
            maxage: None,
            timeout: UpstreamConfig::DEFAULT_TIMEOUT,
            max_fails,
            fail_timeout: Duration::from_mins(5),
            cache: true,
            search: false,
            access: None,
            rules: pnpr_policy::PackageRules::default(),
        },
    )
}

async fn assert_redirect_timeout(delay_body: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut request = [0u8; 4096];
        let (mut first, _) = listener.accept().await.unwrap();
        assert!(first.read(&mut request).await.unwrap() > 0);
        tokio::time::sleep(Duration::from_millis(300)).await;
        first.write_all(b"HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
        drop(first);
        let (mut second, _) = listener.accept().await.unwrap();
        assert!(second.read(&mut request).await.unwrap() > 0);
        if delay_body {
            second.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n").await.unwrap();
        }
        tokio::time::sleep(Duration::from_millis(450)).await;
        if !delay_body {
            second.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n").await.unwrap();
        }
        second.write_all(b"body").await.unwrap();
    });
    let mut config = UpstreamConfig::with_defaults(url.clone(), HeaderMap::new());
    config.timeout = Duration::from_millis(600);
    let upstream = Upstream::new("test", &config);
    let result = upstream.fetch_artifact_response(&url).await;
    if delay_body {
        let FetchOutcome::Ok(response) = result.unwrap() else {
            panic!("expected artifact response")
        };
        assert!(response.bytes().await.unwrap_err().is_timeout());
    } else {
        assert!(
            matches!(result, Err(RegistryError::Upstream { source, .. }) if source.is_timeout()),
        );
    }
    server.abort();
}
