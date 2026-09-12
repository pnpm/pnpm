use super::{
    Duration, NetworkSettings, PerRegistryTls, ProxyConfig, ThrottledClient, TlsConfig,
    client_with_fetch_timeout, drain_until_timed_out,
};

/// Regression test for <https://github.com/pnpm/pnpm/issues/14604>.
#[tokio::test]
async fn a_body_that_keeps_arriving_outlives_the_fetch_timeout() {
    const CHUNKS: usize = 6;
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/runtime.tar.gz")
        .with_chunked_body(|writer| {
            for _ in 0..CHUNKS {
                std::thread::sleep(Duration::from_millis(60));
                writer.write_all(b"chunk")?;
            }
            Ok(())
        })
        .create_async()
        .await;
    let client = client_with_fetch_timeout(Duration::from_millis(200));
    let url = format!("{}/runtime.tar.gz", server.url());

    let guard = client.acquire_for_url(&url).await;
    let response = guard.get(&url).send().await.expect("the mock server responds");
    let body = response.bytes().await.expect("a body that keeps arriving must not time out");

    assert_eq!(body.len(), CHUNKS * "chunk".len());
    mock.assert_async().await;
}

#[tokio::test]
async fn a_stalled_body_fails_after_the_fetch_timeout() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/runtime.tar.gz")
        .with_chunked_body(|writer| {
            writer.write_all(b"chunk")?;
            std::thread::sleep(Duration::from_millis(600));
            Ok(())
        })
        .create_async()
        .await;
    let client = client_with_fetch_timeout(Duration::from_millis(100));
    let url = format!("{}/runtime.tar.gz", server.url());

    let guard = client.acquire_for_url(&url).await;
    let response = guard.get(&url).send().await.expect("the mock server responds");
    let error = response.bytes().await.expect_err("a stalled body must time out");

    assert!(error.is_timeout(), "got {error:?}");
}

#[test]
fn for_installs_falls_back_on_unencodable_user_agent() {
    // A user-agent containing a control character cannot be encoded as
    // an HTTP header value; the client must still build (falling back
    // to the default UA) rather than erroring.
    let settings =
        NetworkSettings { user_agent: "bad\nua".to_string(), ..NetworkSettings::default() };
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &settings,
    )
    .expect("unencodable user-agent falls back to default");
}

/// Pins the floor and cap of the default request-concurrency formula.
/// The floor exists because downloads are I/O-bound: deriving it from
/// the core count left low-core CI runners draining multi-hundred-
/// tarball installs 16 requests at a time without saturating a
/// low-latency registry.
#[test]
fn default_network_concurrency_stays_within_floor_and_cap() {
    let concurrency = super::super::default_network_concurrency();
    assert!((64..=96).contains(&concurrency), "got {concurrency}");
}

/// A redirect to a host the guard rejects must fail the request before the
/// off-allowlist target is ever contacted — the redirect SSRF boundary.
#[tokio::test]
async fn redirect_guard_blocks_off_allowlist_redirect_target() {
    let mut server = mockito::Server::new_async().await;
    let redirect = server
        .mock("GET", "/pkg")
        .with_status(302)
        .with_header("location", "http://169.254.169.254/internal")
        .create_async()
        .await;

    // Allow only the entry server's own origin, never the redirect target.
    let allowed = format!("{}/", server.url());
    let client = ThrottledClient::new_for_installs_with_redirect_guard(move |url| {
        url.as_str().starts_with(&allowed)
    });
    let guard = client.acquire().await;
    let result = guard.get(format!("{}/pkg", server.url())).send().await;

    redirect.assert_async().await;
    assert!(result.is_err(), "a redirect to an off-allowlist host must be blocked");
}

/// A redirect whose target the guard allows is followed normally, so an
/// allowlisted registry that legitimately redirects keeps working.
#[tokio::test]
async fn redirect_guard_follows_allowlisted_redirect_target() {
    let mut target = mockito::Server::new_async().await;
    let body = target.mock("GET", "/final").with_status(200).with_body("ok").create_async().await;
    let mut entry = mockito::Server::new_async().await;
    let redirect = entry
        .mock("GET", "/pkg")
        .with_status(302)
        .with_header("location", &format!("{}/final", target.url()))
        .create_async()
        .await;

    let entry_origin = format!("{}/", entry.url());
    let target_origin = format!("{}/", target.url());
    let client = ThrottledClient::new_for_installs_with_redirect_guard(move |url| {
        let url = url.as_str();
        url.starts_with(&entry_origin) || url.starts_with(&target_origin)
    });
    let guard = client.acquire().await;
    let resp = guard.get(format!("{}/pkg", entry.url())).send().await.expect("redirect followed");

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.expect("body"), "ok");
    redirect.assert_async().await;
    body.assert_async().await;
}

/// `maxSockets` caps concurrent in-flight requests to a single origin: a
/// second request to the same origin blocks while the first guard is held,
/// but a request to a different origin is unaffected.
#[tokio::test]
async fn max_sockets_caps_concurrent_sockets_per_origin() {
    use std::time::Duration;

    let client = ThrottledClient::new_for_installs().with_max_sockets_per_host(Some(1));
    let held = client.acquire_for_url("https://registry.example.com/a").await;

    // A second socket to the same origin must wait for `held` to drop.
    let blocked = tokio::time::timeout(
        Duration::from_millis(150),
        client.acquire_for_url("https://registry.example.com/b"),
    )
    .await;
    assert!(blocked.is_err(), "second socket to the same origin should block under maxSockets=1");

    // A different origin has its own budget and is not blocked.
    tokio::time::timeout(
        Duration::from_millis(150),
        client.acquire_for_url("https://other.example.com/a"),
    )
    .await
    .expect("a different origin should not be blocked");

    // Releasing the first guard frees the origin's single slot.
    drop(held);
    tokio::time::timeout(
        Duration::from_millis(150),
        client.acquire_for_url("https://registry.example.com/c"),
    )
    .await
    .expect("the origin's slot should be free after the first guard drops");
}

/// Without a `maxSockets` cap, many concurrent requests to one origin all
/// acquire immediately (bounded only by the global concurrency semaphore).
#[tokio::test]
async fn no_max_sockets_leaves_per_origin_uncapped() {
    use std::time::Duration;

    let client = ThrottledClient::new_for_installs();
    let _g1 = client.acquire_for_url("https://registry.example.com/a").await;
    tokio::time::timeout(
        Duration::from_millis(150),
        client.acquire_for_url("https://registry.example.com/b"),
    )
    .await
    .expect("a second socket to the same origin should not block without maxSockets");
}

#[tokio::test]
async fn stalled_consumers_release_permits_on_deadline_or_cancellation() {
    use futures_util::StreamExt;

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/artifact")
        .with_body(vec![0; 2 * 1024 * 1024])
        .expect(2)
        .create_async()
        .await;
    let url = format!("{}/artifact", server.url());
    let client = ThrottledClient::new_for_installs().with_max_sockets_per_host(Some(1));
    let initial_permits = client.semaphore.available_permits();
    for cancel in [false, true] {
        let guard = client.acquire_for_url(&url).await;
        let response = guard.get(&url).send().await.unwrap();
        let budget = if cancel { Duration::from_secs(30) } else { Duration::from_millis(100) };
        let mut stream = Box::pin(guard.retain_for_body(response, budget).bytes_stream());
        stream.next().await.unwrap().unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), client.acquire_for_url(&url))
                .await
                .is_err(),
        );
        if cancel {
            drop(stream);
        } else {
            let guard = tokio::time::timeout(Duration::from_secs(1), client.acquire_for_url(&url))
                .await
                .expect("deadline releases a stalled stream's permits");
            drop(guard);
            drain_until_timed_out(&mut stream).await;
        }
        let guard = tokio::time::timeout(Duration::from_secs(1), client.acquire_for_url(&url))
            .await
            .unwrap();
        drop(guard);
        assert_eq!(client.semaphore.available_permits(), initial_permits);
    }
    mock.assert_async().await;
}
