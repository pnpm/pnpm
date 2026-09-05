use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use reqwest::StatusCode;

use super::{RetryOpts, SecureAttemptError, get_secure_bytes, retry_async, should_retry_status};
use crate::{
    AuthHeaders, PerRegistryTls, ProxyConfig, SecureAuthResponse, ThrottledClient, TlsConfig,
    nerf_dart,
};

/// `RetryOpts` whose backoff is effectively instant, so retry-loop
/// tests don't sleep.
fn instant_retry_opts(retries: u32) -> RetryOpts {
    RetryOpts {
        retries,
        factor: 1,
        min_timeout: Duration::from_millis(1),
        max_timeout: Duration::from_millis(1),
    }
}

#[tokio::test]
async fn manual_metadata_redirects_preserve_the_configured_guard() {
    for allow in [false, true] {
        eprintln!("allow={allow}");
        let mut source = mockito::Server::new_async().await;
        let mut target = mockito::Server::new_async().await;
        let destination = target
            .mock("GET", "/metadata")
            .with_body("ok")
            .expect(usize::from(allow))
            .create_async()
            .await;
        let redirect = source
            .mock("GET", "/start")
            .with_status(302)
            .with_header("location", &format!("{}/metadata", target.url()))
            .expect(1)
            .create_async()
            .await;
        let client = ThrottledClient::new_for_installs_with_redirect_guard(move |_| allow);
        let result = get_secure_bytes(
            &client,
            &format!("{}/start", source.url()),
            &AuthHeaders::default(),
            None,
            instant_retry_opts(2),
            1024,
        )
        .await;
        if allow {
            assert_eq!(result.expect("allowed redirect succeeds").body, b"ok");
        } else {
            let error = result.err().expect("blocked redirect fails");
            eprintln!("error={error:?}");
            assert!(error.is_redirect());
            assert!(error.to_string().contains("redirect"));
        }
        redirect.assert_async().await;
        destination.assert_async().await;
    }
}

#[tokio::test]
async fn authenticated_metadata_errors_remove_urls_after_retry_exhaustion() {
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    let url = format!("http://user:password@{address}/metadata?token=secret#fragment");
    let error = get_secure_bytes(
        &ThrottledClient::default(),
        &url,
        &AuthHeaders::default(),
        None,
        instant_retry_opts(1),
        1024,
    )
    .await
    .err()
    .expect("closed metadata endpoint must fail");
    eprintln!("error={error:?}");
    assert!(error.is_connect());
    assert!(error.url().is_none());
    for secret in ["password", "token", "secret", "fragment"] {
        assert!(!error.to_string().contains(secret));
    }
}

#[tokio::test]
async fn maximum_retry_budget_does_not_overflow_logging_counters() {
    let mut registry = mockito::Server::new_async().await;
    let failure = registry.mock("GET", "/metadata").with_status(503).expect(1).create_async().await;
    let success = registry.mock("GET", "/metadata").with_body("ok").expect(1).create_async().await;
    let url = format!("{}/metadata", registry.url());
    let client = ThrottledClient::default();
    let response = crate::send_with_retry(&client, &url, instant_retry_opts(u32::MAX), |client| {
        client.get(&url)
    })
    .await
    .unwrap();
    eprintln!("response={:?}", response.1);
    assert_eq!(response.1.status(), StatusCode::OK);
    failure.assert_async().await;
    success.assert_async().await;
    let attempts = AtomicU32::new(0);
    retry_async(
        &url,
        instant_retry_opts(u32::MAX),
        |(): &()| true,
        || async { if attempts.fetch_add(1, Ordering::Relaxed) == 0 { Err(()) } else { Ok(()) } },
    )
    .await
    .unwrap();
    assert_eq!(attempts.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn authenticated_metadata_uses_the_shared_status_policy_and_preserves_final_response() {
    for (status, attempts) in [(408, 3), (429, 3), (503, 3), (401, 1), (403, 1), (404, 1), (200, 1)]
    {
        eprintln!("status={status}, attempts={attempts}");
        let mut registry = mockito::Server::new_async().await;
        let request = registry
            .mock("GET", "/metadata")
            .with_status(status)
            .with_body("final response")
            .expect(attempts)
            .create_async()
            .await;
        let url = format!("{}/metadata", registry.url());
        let response = get_secure_bytes(
            &ThrottledClient::default(),
            &url,
            &AuthHeaders::default(),
            None,
            instant_retry_opts(2),
            usize::MAX,
        )
        .await
        .unwrap();
        assert_eq!(response.status.as_u16(), status as u16);
        assert_eq!(response.body, b"final response");
        assert_eq!(response.url, url);
        request.assert_async().await;
    }
}

#[tokio::test]
async fn metadata_retry_restarts_redirects_without_forwarding_origin_credentials() {
    let mut target = mockito::Server::new_async().await;
    let redirected = target
        .mock("GET", "/metadata")
        .match_header("authorization", mockito::Matcher::Missing)
        .match_header("accept", "application/example+json")
        .with_status(503)
        .expect(3)
        .create_async()
        .await;
    let mut origin = mockito::Server::new_async().await;
    let initial = origin
        .mock("GET", "/start")
        .match_header("authorization", "Bearer origin-only")
        .with_status(302)
        .with_header("location", &format!("{}/metadata", target.url()))
        .expect(3)
        .create_async()
        .await;
    let auth =
        AuthHeaders::from_creds_map([(nerf_dart(&origin.url()), "Bearer origin-only".to_string())]);
    let client = ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &crate::NetworkSettings { network_concurrency: 1, ..Default::default() },
    )
    .unwrap();
    let response = get_secure_bytes(
        &client,
        &format!("{}/start", origin.url()),
        &auth,
        Some("application/example+json"),
        instant_retry_opts(2),
        usize::MAX,
    )
    .await
    .unwrap();
    assert_eq!(response.status, 503);
    initial.assert_async().await;
    redirected.assert_async().await;
}

#[test]
fn metadata_retry_diagnostics_do_not_include_response_body_or_url() {
    let error = SecureAttemptError::Response(SecureAuthResponse {
        status: StatusCode::SERVICE_UNAVAILABLE,
        body: b"secret response".to_vec(),
        body_truncated: false,
        url: "https://example.test/private-token".to_string(),
    });
    let debug = format!("{error:?}");
    eprintln!("diagnostic: {debug}");
    assert!(!debug.contains("secret"));
    assert!(!debug.contains("private-token"));
    assert!(debug.contains("503"));
}

#[tokio::test]
async fn metadata_retry_recovers_an_interrupted_response_body() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/metadata", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for response in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\npart".as_slice(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".as_slice(),
        ] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = socket.read(&mut buffer).await.unwrap();
                assert_ne!(count, 0, "request ended before its headers: {request:?}");
                request.extend_from_slice(&buffer[..count]);
            }
            socket.write_all(response).await.unwrap();
            socket.shutdown().await.unwrap();
        }
    });
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        get_secure_bytes(
            &ThrottledClient::default(),
            &url,
            &AuthHeaders::default(),
            None,
            instant_retry_opts(1),
            usize::MAX,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.body, b"ok");
    server.await.unwrap();
}

#[test]
fn default_matches_pnpm_fetch_retries() {
    let opts = RetryOpts::default();
    assert_eq!(opts.retries, 2);
    assert_eq!(opts.factor, 10);
    assert_eq!(opts.min_timeout, Duration::from_secs(10));
    assert_eq!(opts.max_timeout, Duration::from_mins(1));
}

#[tokio::test]
async fn bounded_metadata_stops_at_limit_without_retrying_oversized_bodies() {
    for status in [200, 503, 302] {
        for chunked in [false, true] {
            eprintln!("status={status}, chunked={chunked}");
            let mut server = mockito::Server::new_async().await;
            let request = server.mock("GET", "/metadata").with_status(status).expect(1);
            let request = if chunked {
                request.with_chunked_body(|writer| writer.write_all(b"0123456789abcdefEXCESS"))
            } else {
                request.with_body("0123456789abcdefEXCESS")
            }
            .create_async()
            .await;
            let response = ThrottledClient::default()
                .get_limited_bytes_with_secure_auth_and_retry(
                    &format!("{}/metadata", server.url()),
                    &AuthHeaders::default(),
                    None,
                    instant_retry_opts(2),
                    16,
                )
                .await
                .unwrap();
            assert_eq!(response.body, b"0123456789abcdef");
            assert!(response.body_truncated, "oversized response was accepted");
            assert_eq!(response.status.as_u16(), status as u16);
            request.assert_async().await;
        }
    }
}

#[tokio::test]
async fn bounded_metadata_accepts_exact_limit_after_redirect() {
    let mut server = mockito::Server::new_async().await;
    let initial = server
        .mock("GET", "/start")
        .with_status(302)
        .with_header("location", "/metadata")
        .expect(1)
        .create_async()
        .await;
    let request = server
        .mock("GET", "/metadata")
        .with_body("0123456789abcdef")
        .expect(1)
        .create_async()
        .await;
    let response = ThrottledClient::default()
        .get_limited_bytes_with_secure_auth_and_retry(
            &format!("{}/start", server.url()),
            &AuthHeaders::default(),
            None,
            instant_retry_opts(2),
            16,
        )
        .await
        .unwrap();
    assert_eq!(response.body, b"0123456789abcdef");
    assert!(!response.body_truncated, "exact-limit response was rejected");
    assert_eq!(response.url, format!("{}/metadata", server.url()));
    initial.assert_async().await;
    request.assert_async().await;
}

#[test]
fn delay_for_grows_exponentially_then_caps_at_max() {
    let opts = RetryOpts {
        retries: 5,
        factor: 10,
        min_timeout: Duration::from_secs(1),
        max_timeout: Duration::from_mins(1),
    };
    assert_eq!(opts.delay_for(0), Duration::from_secs(1), "first wait is min_timeout");
    assert_eq!(opts.delay_for(1), Duration::from_secs(10), "min * factor^1");
    // min * factor^2 = 100_000 ms, capped to max_timeout.
    assert_eq!(opts.delay_for(2), Duration::from_mins(1), "capped at max_timeout");
}

#[test]
fn delay_for_saturates_instead_of_overflowing() {
    let opts = RetryOpts {
        retries: 100,
        factor: 10,
        min_timeout: Duration::from_millis(1),
        max_timeout: Duration::from_millis(u64::MAX),
    };
    // factor.pow(50) overflows u64; saturate to the largest expressible
    // delay rather than wrapping or panicking.
    assert_eq!(opts.delay_for(50), Duration::from_millis(u64::MAX));
}

#[test]
fn retryable_statuses_match_pnpm() {
    assert!(should_retry_status(StatusCode::REQUEST_TIMEOUT)); // 408
    assert!(should_retry_status(StatusCode::TOO_MANY_REQUESTS)); // 429
    assert!(should_retry_status(StatusCode::INTERNAL_SERVER_ERROR)); // 500
    assert!(should_retry_status(StatusCode::BAD_GATEWAY)); // 502
    assert!(should_retry_status(StatusCode::SERVICE_UNAVAILABLE)); // 503

    assert!(!should_retry_status(StatusCode::OK)); // 200
    assert!(!should_retry_status(StatusCode::NOT_MODIFIED)); // 304
    assert!(!should_retry_status(StatusCode::UNAUTHORIZED)); // 401
    assert!(!should_retry_status(StatusCode::FORBIDDEN)); // 403
    assert!(!should_retry_status(StatusCode::NOT_FOUND)); // 404
}

#[tokio::test]
async fn retry_async_retries_a_retryable_error_until_success() {
    let calls = AtomicU32::new(0);
    let result: Result<&str, &str> = retry_async(
        "https://registry/pkg",
        instant_retry_opts(3),
        |_error| true,
        || {
            let attempt = calls.fetch_add(1, Ordering::Relaxed);
            async move { if attempt < 2 { Err("error decoding response body") } else { Ok("ok") } }
        },
    )
    .await;
    assert_eq!(result, Ok("ok"));
    assert_eq!(calls.load(Ordering::Relaxed), 3, "two failures then a success");
}

#[tokio::test]
async fn retry_async_does_not_retry_a_non_retryable_error() {
    let calls = AtomicU32::new(0);
    let result: Result<(), &str> = retry_async(
        "https://registry/pkg",
        instant_retry_opts(3),
        |_error| false,
        || {
            calls.fetch_add(1, Ordering::Relaxed);
            async { Err("fatal") }
        },
    )
    .await;
    assert_eq!(result, Err("fatal"));
    let total = calls.load(Ordering::Relaxed);
    assert_eq!(total, 1, "non-retryable errors return on the first attempt");
}

#[tokio::test]
async fn retry_async_gives_up_after_the_retry_budget() {
    let calls = AtomicU32::new(0);
    let result: Result<(), &str> = retry_async(
        "https://registry/pkg",
        instant_retry_opts(2),
        |_error| true,
        || {
            calls.fetch_add(1, Ordering::Relaxed);
            async { Err("error decoding response body") }
        },
    )
    .await;
    assert_eq!(result, Err("error decoding response body"));
    assert_eq!(calls.load(Ordering::Relaxed), 3, "initial attempt plus `retries` retries");
}
