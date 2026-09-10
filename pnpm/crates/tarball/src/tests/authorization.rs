use super::{
    AuthHeaders, FASTIFY_ERROR_INTEGRITY, FASTIFY_ERROR_TARBALL, FetchTarballForResolution,
    HttpStatusError, SilentReporter, TarballError, ThrottledClient, assert_eq,
    auth_header_for_package_download, fast_retry_opts, fetch_and_extract_with_retry, integrity,
    tempdir_with_leaked_path,
};

#[test]
fn node_runtime_downloads_do_not_send_auth_over_remote_http() {
    let auth_headers = AuthHeaders::from_creds_map([(
        "//mirror.example/".to_string(),
        "Bearer mirror-token".to_string(),
    )]);
    assert_eq!(
        auth_header_for_package_download(
            &auth_headers,
            "http://mirror.example/node.tar.gz",
            "node@runtime:22.0.0",
        ),
        None,
    );
    assert_eq!(
        auth_header_for_package_download(
            &auth_headers,
            "https://mirror.example/node.tar.gz",
            "node@runtime:22.0.0",
        )
        .as_deref(),
        Some("Bearer mirror-token"),
    );
}

/// `FetchTarballForResolution` must forward its `package_id` (the package's
/// `name@version`) for auth/scope selection, so a private scoped registry tarball
/// resolves its scope token while its integrity is computed during resolution.
#[tokio::test]
async fn fetch_for_resolution_uses_package_id_for_scoped_auth() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .match_header("authorization", "Bearer scoped-token")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let registry_key = format!("{}@scope", pnpm_network::nerf_dart(&server.url()));
    let auth_headers =
        AuthHeaders::from_creds_map([(registry_key, "Bearer scoped-token".to_owned())]);

    let resolved = FetchTarballForResolution {
        http_client: &client,
        store_dir: store_path,
        store_index_writer: None,
        package_url: &url,
        package_id: "@scope/test-pkg@1.0.0",
        auth_headers: &auth_headers,
        retry_opts: fast_retry_opts(),
        manifest_subdir: None,
    }
    .run::<SilentReporter>(None)
    .await
    .expect("the scope token selected via package_id should let the fetch succeed");

    assert_eq!(resolved.integrity, integrity(FASTIFY_ERROR_INTEGRITY));
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// When [`AuthHeaders`] resolves a credential for the tarball URL,
/// the GET request must carry the `Authorization` header — including
/// for tarball hosts that differ from the metadata host.
/// `mockito::Matcher::Exact` rejects the request unless the header
/// matches verbatim, so a missing or wrong header would 501 the
/// request and fail the integrity check downstream.
#[tokio::test]
async fn fetch_attaches_authorization_header_when_creds_match_tarball_url() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let auth_headers = AuthHeaders::from_creds_map([(
        pnpm_network::nerf_dart(&url),
        "Bearer test-token".to_owned(),
    )]);

    let (_integrity, cas_paths, _idx) = fetch_and_extract_with_retry::<SilentReporter>(
        &client,
        &url,
        Some(&pkg_integrity),
        None,
        0,
        "test-pkg",
        "",
        store_path,
        fast_retry_opts(),
        &auth_headers,
        None,
        None,
        false,
    )
    .await
    .expect("server should accept the request once the bearer header is attached");

    assert!(cas_paths.contains_key("package.json"));
    mock.assert_async().await;
    drop(store_dir_keep);
}

#[tokio::test]
async fn fetch_attaches_authorization_header_when_scope_creds_match_package_id() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .match_header("authorization", "Bearer scoped-token")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let registry_key = format!("{}@scope", pnpm_network::nerf_dart(&server.url()));
    let auth_headers =
        AuthHeaders::from_creds_map([(registry_key, "Bearer scoped-token".to_owned())]);

    let (_integrity, cas_paths, _idx) = fetch_and_extract_with_retry::<SilentReporter>(
        &client,
        &url,
        Some(&pkg_integrity),
        None,
        0,
        "@scope/test-pkg@1.0.0",
        "",
        store_path,
        fast_retry_opts(),
        &auth_headers,
        None,
        None,
        false,
    )
    .await
    .expect("server should accept the request once the scoped bearer header is attached");

    assert!(cas_paths.contains_key("package.json"));
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// The retry loop must re-attach the `Authorization` header on every
/// attempt, not just the first. A regression that read `auth_headers`
/// once outside the loop would pass the single-attempt test
/// [`fetch_attaches_authorization_header_when_creds_match_tarball_url`]
/// but silently 401 on the retried call. Mock returns 503 then 200,
/// both gated on the bearer header.
#[tokio::test]
async fn retry_re_attaches_authorization_header_on_each_attempt() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let fail = server
        .mock("GET", "/pkg.tgz")
        .match_header("authorization", "Bearer test-token")
        .with_status(503)
        .expect(1)
        .create_async()
        .await;
    let ok = server
        .mock("GET", "/pkg.tgz")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let auth_headers = AuthHeaders::from_creds_map([(
        pnpm_network::nerf_dart(&url),
        "Bearer test-token".to_owned(),
    )]);

    let (_integrity, cas_paths, _idx) = fetch_and_extract_with_retry::<SilentReporter>(
        &client,
        &url,
        Some(&pkg_integrity),
        None,
        0,
        "test-pkg",
        "",
        store_path,
        fast_retry_opts(),
        &auth_headers,
        None,
        None,
        false,
    )
    .await
    .expect("retry attempt should also carry the bearer header");

    assert!(cas_paths.contains_key("package.json"));
    // Both mocks must have fired: header missing on the retry would
    // mean the second `match_header` rejects (501) and the test fails
    // either at this assertion or at the integrity check.
    fail.assert_async().await;
    ok.assert_async().await;
    drop(store_dir_keep);
}

/// A tarball URL can carry inline `user:pass@` credentials — typed on the
/// command line for `pnpm add <url>`, or declared in a manifest — and every
/// error rendering the URL lands in terminal scrollback and CI logs.
#[test]
fn url_bearing_errors_redact_inline_credentials() {
    let url = "https://alice:hunter2@example.com/pkg.tgz".to_string();
    let rendered = [
        TarballError::HttpStatus(HttpStatusError { url: url.clone(), status: 404 }).to_string(),
        TarballError::TarballTooLarge { url: url.clone(), advertised_size: u64::MAX }.to_string(),
        TarballError::SiblingFetchFailed { url: url.clone() }.to_string(),
        TarballError::OffAllowlist { url }.to_string(),
    ];
    for message in rendered {
        eprintln!("MESSAGE: {message}");
        assert!(!message.contains("hunter2"), "the password must not be rendered: {message}");
        assert!(message.contains("example.com/pkg.tgz"), "the host must survive: {message}");
    }
}
