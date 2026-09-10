use super::{
    AsyncReadExt, AsyncWriteExt, CacheValidators, CanonicalPackageName, CircuitBreaker, Duration,
    RegistryError, TcpListener, breaking_upstream,
};

#[tokio::test]
async fn discovery_body_read_failure_opens_the_circuit() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        let _ = socket.read(&mut request).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\n\
                  Content-Length: 1024\r\n\
                  Content-Type: application/json\r\n\
                  Connection: close\r\n\
                  \r\n\
                  {}",
            )
            .await
            .unwrap();
    });
    let upstream = breaking_upstream(url, 1);

    let first = upstream.fetch_search("text=foo").await;
    server.await.unwrap();
    assert!(matches!(first, Err(RegistryError::UpstreamResponse { .. })));
    assert!(matches!(
        upstream.fetch_search("text=foo").await,
        Err(RegistryError::UpstreamUnavailable { .. }),
    ));
}

#[test]
fn circuit_breaker_opens_after_max_fails_and_resets_on_success() {
    let breaker = CircuitBreaker::new(2, Duration::from_mins(5));
    assert!(breaker.try_acquire());
    breaker.record_failure();
    assert!(breaker.try_acquire(), "one failure is under the threshold");
    breaker.record_failure();
    assert!(!breaker.try_acquire(), "two failures trip the breaker for the cooldown");
    breaker.record_success();
    assert!(breaker.try_acquire(), "a success clears the failure count");
}

#[test]
fn circuit_breaker_reopens_once_cooldown_elapses() {
    // A zero cooldown means a tripped breaker is immediately retryable —
    // the half-open probe path.
    let breaker = CircuitBreaker::new(1, Duration::ZERO);
    breaker.record_failure();
    assert!(breaker.try_acquire(), "a zero fail_timeout lets the next probe through");
}

#[test]
fn circuit_breaker_admits_one_probe_per_cooldown_window() {
    // A short but non-zero cooldown so we can drive the half-open window
    // deterministically with a sleep.
    let cooldown = Duration::from_millis(40);
    let breaker = CircuitBreaker::new(1, cooldown);
    breaker.record_failure();
    assert!(!breaker.try_acquire(), "still cooling down right after the failure");

    std::thread::sleep(cooldown + Duration::from_millis(20));
    assert!(breaker.try_acquire(), "the first caller after the cooldown probes");
    assert!(!breaker.try_acquire(), "admitting the probe re-armed the window; others wait");

    // A probe that never reports back (cancelled mid-request) must not
    // stick the breaker open forever: once the window lapses the next
    // caller probes again.
    std::thread::sleep(cooldown + Duration::from_millis(20));
    assert!(breaker.try_acquire(), "an abandoned probe self-heals after the cooldown");

    // A successful probe closes the breaker entirely.
    breaker.record_success();
    assert!(breaker.try_acquire(), "a closed breaker admits everyone");
}

#[test]
fn circuit_breaker_disabled_when_max_fails_is_zero() {
    let breaker = CircuitBreaker::new(0, Duration::from_mins(5));
    breaker.record_failure();
    breaker.record_failure();
    assert!(breaker.try_acquire(), "max_fails == 0 disables the breaker");
}

#[tokio::test]
async fn open_circuit_short_circuits_without_hitting_the_upstream() {
    let mut server = mockito::Server::new_async().await;
    // `max_fails: 1` trips after the first 500; the breaker must then
    // short-circuit, so the upstream is hit exactly once.
    let mock = server.mock("GET", "/foo").with_status(500).expect(1).create_async().await;

    let upstream = breaking_upstream(server.url(), 1);
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();

    let first = upstream.fetch_packument(&name, &CacheValidators::default()).await;
    assert!(matches!(first, Err(RegistryError::UpstreamStatus { status: 500, .. })));

    let second = upstream.fetch_packument(&name, &CacheValidators::default()).await;
    assert!(
        matches!(second, Err(RegistryError::UpstreamUnavailable { .. })),
        "the open breaker must short-circuit the second request",
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn client_error_status_does_not_open_the_circuit() {
    let mut server = mockito::Server::new_async().await;
    // A 401 is an authoritative answer, not an availability failure: even
    // at `max_fails: 1` it must not trip the breaker, so the upstream is
    // reached on both requests rather than masked behind a 503.
    let mock = server.mock("GET", "/foo").with_status(401).expect(2).create_async().await;

    let upstream = breaking_upstream(server.url(), 1);
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();

    for _ in 0..2 {
        let result = upstream.fetch_packument(&name, &CacheValidators::default()).await;
        assert!(
            matches!(result, Err(RegistryError::UpstreamStatus { status: 401, .. })),
            "a 4xx must surface verbatim, not as a circuit-open 503",
        );
    }
    mock.assert_async().await;
}
