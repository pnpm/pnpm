use super::{
    AsyncReadExt, AsyncWriteExt, CanonicalPackageName, Duration, FetchOutcome, HeaderMap,
    TcpListener, Upstream, UpstreamConfig, assert_redirect_chain_shares_one_head_deadline,
    auth_and_custom_headers, upstream,
};
use futures_util::StreamExt;

#[tokio::test]
async fn fetch_artifact_response_sends_headers_only_to_the_upstream_origin() {
    let mut index = mockito::Server::new_async().await;
    let mut files = mockito::Server::new_async().await;
    let same_origin = index
        .mock("GET", "/dl/serde/1.0.0")
        .match_header("authorization", "Bearer secret-token")
        .with_body("crate bytes")
        .expect(1)
        .create_async()
        .await;
    let other_origin = files
        .mock("GET", "/packages/x.whl")
        .match_header("authorization", mockito::Matcher::Missing)
        .match_header("x-org", mockito::Matcher::Missing)
        .with_body("wheel bytes")
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(index.url(), auth_and_custom_headers());
    let response = upstream
        .fetch_artifact_response(&format!("{}/dl/serde/1.0.0", index.url()))
        .await
        .unwrap();
    let FetchOutcome::Ok(response) = response else { panic!("expected a response") };
    assert_eq!(response.bytes().await.unwrap(), "crate bytes");
    let response = upstream
        .fetch_artifact_response(&format!("{}/packages/x.whl", files.url()))
        .await
        .unwrap();
    let FetchOutcome::Ok(response) = response else { panic!("expected a response") };
    assert_eq!(response.bytes().await.unwrap(), "wheel bytes");
    same_origin.assert_async().await;
    other_origin.assert_async().await;
}

#[tokio::test]
async fn artifact_fetch_guard_rejects_initial_urls_and_redirects() {
    let mut source = mockito::Server::new_async().await;
    let mut target = mockito::Server::new_async().await;
    let target_mock = target
        .mock("GET", "/artifact")
        .expect(0)
        .create_async()
        .await;
    let redirect = source
        .mock("GET", "/artifact")
        .with_status(302)
        .with_header("location", &format!("{}/artifact", target.url()))
        .expect(1)
        .create_async()
        .await;
    let allowed = reqwest::Url::parse(&source.url()).unwrap().origin();
    let upstream = upstream(source.url(), HeaderMap::new())
        .with_fetch_guard(std::sync::Arc::new(move |url| url.origin() == allowed));
    assert!(
        upstream
            .fetch_artifact_response(&format!("{}/artifact", target.url()))
            .await
            .is_err(),
    );
    assert!(
        upstream
            .fetch_artifact_response(&format!("{}/artifact", source.url()))
            .await
            .is_err(),
    );
    target_mock.assert_async().await;
    redirect.assert_async().await;
}

#[tokio::test]
async fn approved_artifact_redirects_rebuild_headers_for_each_origin() {
    let mut source = mockito::Server::new_async().await;
    let mut cdn = mockito::Server::new_async().await;
    let source_mock = source
        .mock("GET", "/artifact")
        .match_header("authorization", "Bearer secret-token")
        .match_header("x-org", "acme")
        .with_status(302)
        .with_header("location", &format!("{}/redirect", cdn.url()))
        .expect(1)
        .create_async()
        .await;
    let cdn_redirect = cdn
        .mock("GET", "/redirect")
        .match_header("authorization", mockito::Matcher::Missing)
        .match_header("x-org", mockito::Matcher::Missing)
        .with_status(302)
        .with_header("location", "/artifact")
        .expect(2)
        .create_async()
        .await;
    let artifact = cdn
        .mock("GET", "/artifact")
        .match_header("authorization", mockito::Matcher::Missing)
        .match_header("x-org", mockito::Matcher::Missing)
        .with_body("artifact bytes")
        .expect(2)
        .create_async()
        .await;
    let origins = [&source.url(), &cdn.url()].map(|url| reqwest::Url::parse(url).unwrap().origin());
    let upstream = upstream(source.url(), auth_and_custom_headers())
        .with_fetch_guard(std::sync::Arc::new(move |url| origins.contains(&url.origin())));
    for url in [format!("{}/artifact", source.url()), format!("{}/redirect", cdn.url())] {
        let response = upstream.fetch_artifact_response(&url).await.unwrap();
        let FetchOutcome::Ok(response) = response else { panic!("expected the artifact") };
        assert_eq!(response.bytes().await.unwrap(), "artifact bytes");
    }
    source_mock.assert_async().await;
    cdn_redirect.assert_async().await;
    artifact.assert_async().await;
}

#[tokio::test]
async fn redirect_chain_shares_one_head_deadline() {
    assert_redirect_chain_shares_one_head_deadline().await;
}

/// An upstream that answers one request with `chunks` pieces of a body,
/// `gap` apart, and then holds the connection open.
async fn spawn_slow_body(chunks: usize, gap: Duration) -> (Upstream, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", chunks * 5 + 5);
        socket.write_all(head.as_bytes()).await.unwrap();
        for _ in 0..chunks {
            socket.write_all(b"chunk").await.unwrap();
            tokio::time::sleep(gap).await;
        }
        tokio::time::sleep(Duration::from_mins(1)).await;
    });
    let mut config = UpstreamConfig::with_defaults(url, HeaderMap::new());
    config.requests.timeout = Duration::from_millis(300);
    (Upstream::new("test", &config), server)
}

async fn read_tarball(upstream: &Upstream, expected_chunks: usize) -> std::io::Result<usize> {
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let FetchOutcome::Ok(response) =
        upstream.fetch_tarball_response(&name, "foo.tgz").await.unwrap()
    else {
        panic!("expected a tarball response")
    };
    let mut stream = Box::pin(response.bytes_stream());
    let mut received = 0;
    while received < expected_chunks * 5 {
        received += stream
            .next()
            .await
            .expect("more of the body")?
            .len();
    }
    Ok(received)
}

#[tokio::test]
async fn a_streamed_body_that_keeps_arriving_outlives_the_timeout() {
    let (upstream, server) = spawn_slow_body(8, Duration::from_millis(100)).await;
    let started = std::time::Instant::now();
    let received = read_tarball(&upstream, 8).await.expect("a body that keeps arriving is read");
    assert_eq!(received, 40);
    assert!(started.elapsed() > Duration::from_millis(600), "the body outlived the 300ms timeout");
    server.abort();
}

#[tokio::test]
async fn tarball_head_deadline_starts_after_waiting_for_a_permit() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody").await.unwrap();
    });
    let mut config = UpstreamConfig::with_defaults(url.clone(), HeaderMap::new());
    config.requests.timeout = Duration::from_millis(250);
    let mut upstream = Upstream::new("test", &config);
    upstream.http.client = std::sync::Arc::new(
        pnpm_network::ThrottledClient::new_for_installs().with_max_sockets_per_host(Some(1)),
    );
    let held = upstream.http.client.acquire_for_url(&url).await;
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let release_permit = async {
        tokio::time::sleep(Duration::from_millis(400)).await;
        drop(held);
    };
    let fetch = upstream.fetch_tarball_response(&name, "foo.tgz");
    let (result, ()) = tokio::join!(fetch, release_permit);
    let FetchOutcome::Ok(response) = result.unwrap() else { panic!("expected a tarball response") };
    let mut stream = Box::pin(response.bytes_stream());
    assert_eq!(stream.next().await.unwrap().unwrap(), "body");
    server.await.unwrap();
}

#[tokio::test]
async fn a_redirect_hops_permit_wait_does_not_count_toward_the_head_deadline() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_url = format!("http://{}/final", target.local_addr().unwrap());
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_url = format!("http://{}", origin.local_addr().unwrap());
    let redirect = format!(
        "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    let server = tokio::spawn(async move {
        let mut request = [0u8; 4096];
        let (mut first, _) = origin.accept().await.unwrap();
        assert!(first.read(&mut request).await.unwrap() > 0);
        first.write_all(redirect.as_bytes()).await.unwrap();
        let (mut second, _) = target.accept().await.unwrap();
        assert!(second.read(&mut request).await.unwrap() > 0);
        second.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody").await.unwrap();
    });
    let mut config = UpstreamConfig::with_defaults(origin_url.clone(), HeaderMap::new());
    config.requests.timeout = Duration::from_millis(250);
    let mut upstream = Upstream::new("test", &config);
    upstream.http.client = std::sync::Arc::new(
        pnpm_network::ThrottledClient::new_for_installs().with_max_sockets_per_host(Some(1)),
    );
    let held = upstream.http.client.acquire_for_url(&target_url).await;
    let release_permit = async {
        tokio::time::sleep(Duration::from_millis(400)).await;
        drop(held);
    };
    let fetch = upstream.fetch_artifact_response(&origin_url);
    let (result, ()) = tokio::join!(fetch, release_permit);
    let FetchOutcome::Ok(response) = result.unwrap() else {
        panic!("expected an artifact response")
    };
    let mut stream = Box::pin(response.bytes_stream());
    assert_eq!(stream.next().await.unwrap().unwrap(), "body");
    server.await.unwrap();
}

#[tokio::test]
async fn a_stalled_error_body_is_bounded_by_the_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        let response = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 100\r\n\r\nbroke";
        socket.write_all(response.as_bytes()).await.unwrap();
        tokio::time::sleep(Duration::from_mins(1)).await;
    });
    let mut config = UpstreamConfig::with_defaults(url, HeaderMap::new());
    config.requests.timeout = Duration::from_millis(300);
    let upstream = Upstream::new("test", &config);
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let started = std::time::Instant::now();
    let result = upstream.fetch_tarball_response(&name, "foo.tgz").await;
    assert!(
        matches!(result, Err(pnpr_error::RegistryError::UpstreamStatus { status: 500, .. })),
        "got {result:?}",
    );
    assert!(started.elapsed() < Duration::from_secs(5), "the error body read was not bounded");
    server.abort();
}

#[tokio::test]
async fn a_streamed_body_that_stalls_fails_after_the_timeout() {
    let (upstream, server) = spawn_slow_body(1, Duration::from_mins(1)).await;
    let error = read_tarball(&upstream, 2).await.expect_err("a stalled body times out");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    server.abort();
}

#[tokio::test]
async fn artifact_and_npm_downloads_hold_permits_after_returning_headers() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", mockito::Matcher::Any)
        .with_body("artifact")
        .expect(3)
        .create_async()
        .await;
    let mut upstream = upstream(server.url(), HeaderMap::new());
    upstream.http.client = std::sync::Arc::new(
        pnpm_network::ThrottledClient::new_for_installs().with_max_sockets_per_host(Some(1)),
    );
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    for mode in 0..3 {
        let outcome = match mode {
            0 => upstream.fetch_artifact_response(&server.url()).await,
            1 => upstream.fetch_tarball_response(&name, "foo.tgz").await,
            _ => {
                upstream.fetch_revision_tarball_response(
                    &name,
                    "1.0.0",
                    pnpm_lockfile::TarballRevision::try_from(0).unwrap(),
                )
                .await
            }
        };
        let FetchOutcome::Ok(response) = outcome.unwrap() else {
            panic!("expected artifact response")
        };
        assert!(
            tokio::time::timeout(
                Duration::from_millis(20),
                upstream.http.client.acquire_for_url(&server.url())
            )
            .await
            .is_err(),
        );
        drop(response);
        let guard = tokio::time::timeout(
            Duration::from_secs(1),
            upstream.http.client.acquire_for_url(&server.url()),
        )
        .await
        .unwrap();
        drop(guard);
    }
    mock.assert_async().await;
}

#[tokio::test]
async fn revision_download_budget_starts_after_waiting_for_a_permit() {
    use futures_util::StreamExt;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n").await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        socket.write_all(b"body").await.unwrap();
    });
    let mut config = UpstreamConfig::with_defaults(url.clone(), HeaderMap::new());
    config.requests.timeout = Duration::from_millis(250);
    let mut upstream = Upstream::new("test", &config);
    upstream.http.client = std::sync::Arc::new(
        pnpm_network::ThrottledClient::new_for_installs().with_max_sockets_per_host(Some(1)),
    );
    let held = upstream.http.client.acquire_for_url(&url).await;
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let fetch = upstream.fetch_revision_tarball_response(
        &name,
        "1.0.0",
        pnpm_lockfile::TarballRevision::try_from(0).unwrap(),
    );
    let release_permit = async {
        tokio::time::sleep(Duration::from_millis(400)).await;
        drop(held);
    };
    let (result, ()) = tokio::join!(fetch, release_permit);
    let FetchOutcome::Ok(response) = result.unwrap() else { panic!("expected artifact response") };
    let mut stream = Box::pin(response.bytes_stream());
    assert_eq!(stream.next().await.unwrap().unwrap(), "body");
    assert!(stream.next().await.is_none());
    server.await.unwrap();
}
