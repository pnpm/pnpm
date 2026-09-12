use super::{Duration, ThrottledClient, origin_of};

#[test]
fn origin_of_extracts_scheme_host_and_port() {
    assert_eq!(
        origin_of("https://registry.npmjs.org/is-odd").as_deref(),
        Some("https://registry.npmjs.org"),
    );
    // A non-default port is part of the origin key.
    assert_eq!(
        origin_of("http://localhost:4873/is-odd/-/is-odd-3.0.1.tgz").as_deref(),
        Some("http://localhost:4873"),
    );
    // An explicit scheme-default port normalizes to the same origin as the
    // implicit form, so it cannot fragment the per-origin socket cap.
    assert_eq!(origin_of("https://host/a"), origin_of("https://host:443/a"));
    assert_eq!(origin_of("http://host/a"), origin_of("http://host:80/a"));
    // A non-default port stays distinct.
    assert_ne!(origin_of("https://host/a"), origin_of("https://host:8443/a"));
    // Same host over http vs https are distinct origins.
    assert_ne!(origin_of("http://example.com/a"), origin_of("https://example.com/a"));
    assert_eq!(origin_of("not a url"), None);
}

#[tokio::test]
async fn streamed_responses_retain_both_permits_until_consumed_or_dropped() {
    let mut server = mockito::Server::new_async().await;
    let mock =
        server.mock("GET", "/artifact").with_body("artifact bytes").expect(3).create_async().await;
    let client = ThrottledClient::new_for_installs().with_max_sockets_per_host(Some(1));
    let initial_permits = client.semaphore.available_permits();
    let url = format!("{}/artifact", server.url());
    for mode in 0..3 {
        assert_streamed_response_permits(&client, &url, mode, initial_permits).await;
    }
    mock.assert_async().await;
}

async fn assert_streamed_response_permits(
    client: &ThrottledClient,
    url: &str,
    mode: u8,
    initial_permits: usize,
) {
    use futures_util::StreamExt;

    let guard = client.acquire_for_url(url).await;
    let response = guard.get(url).send().await.unwrap();
    let response = guard.retain_for_body(response, Duration::from_secs(30));
    assert_eq!(response.url().as_str(), url);
    assert_eq!(response.content_length(), Some(14));
    assert_eq!(client.semaphore.available_permits(), initial_permits - 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), client.acquire_for_url(url)).await.is_err(),
    );
    if mode == 0 {
        assert_eq!(response.bytes().await.unwrap(), "artifact bytes");
    } else {
        let mut stream = Box::pin(response.bytes_stream());
        assert_eq!(stream.next().await.unwrap().unwrap(), "artifact bytes");
        if mode == 1 {
            assert!(stream.next().await.is_none());
            assert_eq!(client.semaphore.available_permits(), initial_permits);
        }
        drop(stream);
    }
    let guard =
        tokio::time::timeout(Duration::from_secs(1), client.acquire_for_url(url)).await.unwrap();
    drop(guard);
}
