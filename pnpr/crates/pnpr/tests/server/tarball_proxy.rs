use super::{
    AccessList, AuthState, Body, PackageRules, Request, ServiceExt, StatusCode, TempDir, Value,
    access_rule, body_bytes, body_json, config_for, enable_osv, foo_packument, header, json,
    mock_packument_for_tarball, osv_database, public_cache_pkg, router, router_with_auth,
    sha512_integrity, upstream_endpoint_config,
};

/// The per-package ACL applies to the path-less `dist-tags` reader even when the
/// package routes to an upstream: an unauthorized caller is denied before the
/// upstream is ever queried, so a restricted package's tags don't leak.
#[tokio::test]
async fn upstream_dist_tags_enforce_package_access() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("GET", "/restricted")
        .with_body(
            json!({
                "name": "restricted",
                "dist-tags": { "latest": "1.0.0" },
                "versions": { "1.0.0": { "name": "restricted", "version": "1.0.0" } },
            })
            .to_string(),
        )
        // The ACL must short-circuit the read before any upstream fetch.
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    // The gate lives on the upstream registry's own `packages:` rules: an
    // access-restricted name can't be read even through a public upstream.
    config.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").rules =
        PackageRules::new(vec![access_rule("restricted", "$authenticated")], None);
    let app = router(config);

    let response = app
        .oneshot(Request::get("/-/package/restricted/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    mock.assert_async().await;
}

#[tokio::test]
async fn upstream_auth_and_custom_headers_are_forwarded_upstream() {
    let mut upstream = mockito::Server::new_async().await;
    // The mock only matches when both headers are present, so a passing
    // request proves the resolved per-upstream headers reached upstream.
    let mock = upstream
        .mock("GET", "/foo")
        .match_header("authorization", "Bearer secret-token")
        .match_header("x-org", "acme")
        .with_status(200)
        .with_body(json!({ "name": "foo", "versions": {} }).to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    let upstream = config.upstreams.get_mut("npmjs").expect("default `npmjs` upstream");
    upstream.headers.insert("authorization", "Bearer secret-token".parse().unwrap());
    upstream.headers.insert("x-org", "acme".parse().unwrap());
    // A credentialed upstream must be access-gated (server construction
    // enforces it), so the read authenticates as an admitted caller.
    upstream.access = Some(AccessList::from_tokens(["$authenticated"]));
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(
            Request::get("/foo")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    mock.assert_async().await;
}

#[tokio::test]
async fn tarball_is_proxied_and_cached() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"fake-tarball-bytes";
    let packument_mock = mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", bytes).await;
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), storage.clone()));

    let first = app
        .clone()
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(body_bytes(first.into_body()).await, bytes);

    // A fresh instance over the same storage and origin serves the cached
    // copy; the `expect(1)` mocks prove the upstream is never asked twice.
    let second = router(config_for(&upstream.url(), storage))
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(body_bytes(second.into_body()).await, bytes);

    packument_mock.assert_async().await;
    mock.assert_async().await;
}

#[tokio::test]
async fn upstream_endpoint_serves_packument_with_endpoint_rewritten_tarballs() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": "foo", "version": "1.0.0", "dist": {
            "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
            "integrity": "sha512-deadbeef",
        } } },
    });
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let config = upstream_endpoint_config(&upstream.url(), tmp.path().to_path_buf(), "alice");
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(
            Request::get("/~npmjs/foo")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // Private content must not be cached or replayed by an intermediary.
    assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(), "private, no-store");
    assert_eq!(response.headers().get(header::VARY).unwrap(), "Authorization");
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    // `dist.tarball` is rewritten back onto the same `/~npmjs/` endpoint, so the
    // URL is canonical for the client's configured registry (integrity-only).
    assert_eq!(
        body["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/~npmjs/foo/-/foo-1.0.0.tgz",
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn upstream_endpoint_rejects_unauthorized_caller() {
    let tmp = TempDir::new().unwrap();
    let config = upstream_endpoint_config("http://127.0.0.1:1", tmp.path().to_path_buf(), "alice");
    let app = router(config);
    // Anonymous caller is not admitted by the upstream's access policy, and the
    // request fails closed before any upstream fetch.
    let response =
        app.oneshot(Request::get("/~npmjs/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn upstream_endpoint_tarball_is_verified_and_cached_per_upstream() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"private-upstream-tarball";
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": "foo", "version": "1.0.0", "dist": {
            "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
            "integrity": sha512_integrity(bytes),
        } } },
    });
    // The packument and verified tarball are cached in the upstream's private
    // namespace, so two tarball requests hit the upstream exactly once each —
    // the second is served from the private cache, never re-fetched.
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let config = upstream_endpoint_config(&upstream.url(), tmp.path().to_path_buf(), "alice");
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::get("/~npmjs/foo/-/foo-1.0.0.tgz")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, bytes);
    }

    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn upstream_cache_does_not_leak_to_the_public_path() {
    // The public `**` route proxies through the default `npmjs` upstream to the
    // public upstream; a separate access-bearing `corp` upstream serves a
    // *different* upstream at `/~corp/`. After the private `/~corp/foo` tarball
    // is cached, the public `/foo` request must still serve the public bytes —
    // proving the private upstream's cache is namespaced, not shared.
    let mut public_upstream = mockito::Server::new_async().await;
    let mut private_upstream = mockito::Server::new_async().await;
    let public_bytes = b"public-foo-tarball";
    let private_bytes = b"private-corp-foo-tarball";

    for (server, body) in
        [(&mut public_upstream, &public_bytes[..]), (&mut private_upstream, &private_bytes[..])]
    {
        let packument = json!({
            "name": "foo",
            "dist-tags": { "latest": "1.0.0" },
            "versions": { "1.0.0": { "name": "foo", "version": "1.0.0", "dist": {
                "tarball": format!("{}/foo/-/foo-1.0.0.tgz", server.url()),
                "integrity": sha512_integrity(body),
            } } },
        });
        server.mock("GET", "/foo").with_body(packument.to_string()).create_async().await;
        server
            .mock("GET", "/foo/-/foo-1.0.0.tgz")
            .with_header("content-type", "application/octet-stream")
            .with_body(body)
            .create_async()
            .await;
    }

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&public_upstream.url(), tmp.path().to_path_buf());
    let mut corp = config.upstreams.get("npmjs").expect("default `npmjs` upstream").clone();
    corp.url = private_upstream.url();
    corp.access = Some(AccessList::from_tokens(["alice"]));
    config.upstreams.insert("corp".to_string(), corp);
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);

    // Prime the private cache via the `/~corp/` endpoint.
    let private = app
        .clone()
        .oneshot(
            Request::get("/~corp/foo/-/foo-1.0.0.tgz")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(private.status(), StatusCode::OK);
    assert_eq!(body_bytes(private.into_body()).await, private_bytes);

    // The public path must serve the public upstream's bytes, never the
    // private upstream's cached copy.
    let public = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(public.status(), StatusCode::OK);
    assert_eq!(body_bytes(public.into_body()).await, public_bytes);
}

/// Repointing a registry's `url:` must abandon the previous origin's cache: the
/// namespace is keyed by the origin, so a warm entry fetched from the old
/// upstream — packument or tarball, including via the cache-first tarball
/// path — can never answer for the new one under the same registry name.
#[tokio::test]
async fn repointing_an_upstream_url_abandons_the_old_origins_cache() {
    let mut old_origin = mockito::Server::new_async().await;
    let mut new_origin = mockito::Server::new_async().await;
    let old_bytes = b"old-origin-foo-tarball";
    let new_bytes = b"new-origin-foo-tarball";

    for (server, body) in [(&mut old_origin, &old_bytes[..]), (&mut new_origin, &new_bytes[..])] {
        let packument = json!({
            "name": "foo",
            "dist-tags": { "latest": "1.0.0" },
            "versions": { "1.0.0": { "name": "foo", "version": "1.0.0", "dist": {
                "tarball": format!("{}/foo/-/foo-1.0.0.tgz", server.url()),
                "integrity": sha512_integrity(body),
            } } },
        });
        server.mock("GET", "/foo").with_body(packument.to_string()).create_async().await;
        server
            .mock("GET", "/foo/-/foo-1.0.0.tgz")
            .with_header("content-type", "application/octet-stream")
            .with_body(body)
            .create_async()
            .await;
    }

    // Prime the cache from the old origin.
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&old_origin.url(), tmp.path().to_path_buf()));
    let primed = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(primed.status(), StatusCode::OK);
    assert_eq!(body_bytes(primed.into_body()).await, old_bytes);

    // Same storage, same registry name, new `url:` — the fresh entries must come
    // from the new origin, not the still-fresh cache of the old one.
    let app = router(config_for(&new_origin.url(), tmp.path().to_path_buf()));
    let repointed = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(repointed.status(), StatusCode::OK);
    assert_eq!(body_bytes(repointed.into_body()).await, new_bytes);
}

#[tokio::test]
async fn upstream_endpoint_cache_false_streams_without_caching() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"uncached-private-upstream-tarball";
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": "foo", "version": "1.0.0", "dist": {
            "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
            "integrity": sha512_integrity(bytes),
        } } },
    });
    // A `cache: false` upstream endpoint persists nothing, so each request
    // re-fetches the packument and tarball — the mocks are hit twice.
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_body(packument.to_string())
        .expect(2)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_header("content-type", "application/octet-stream")
        .with_body(bytes)
        .expect(2)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = upstream_endpoint_config(&upstream.url(), tmp.path().to_path_buf(), "alice");
    config.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").cache = false;
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::get("/~npmjs/foo/-/foo-1.0.0.tgz")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, bytes);
    }

    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn osv_refuses_vulnerable_tarball_before_upstream_fetch() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body("vulnerable tarball")
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let osv = osv_database("foo", &["1.0.0"]);
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.resolver.enabled = false;
    enable_osv(&mut config, osv.path());
    let app = router(config);

    let response = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(body.contains("GHSA-registry"), "{body}");

    mock.assert_async().await;
}

#[tokio::test]
async fn osv_tarball_screening_preserves_access_gate() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("GET", "/@pnpm.e2e/needs-auth/-/needs-auth-1.0.0.tgz")
        .with_status(200)
        .with_body("private vulnerable tarball")
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let osv = osv_database("@pnpm.e2e/needs-auth", &["1.0.0"]);
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.resolver.enabled = false;
    enable_osv(&mut config, osv.path());
    let app = router(config);

    let response = app
        .oneshot(
            Request::get("/@pnpm.e2e/needs-auth/-/needs-auth-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    mock.assert_async().await;
}

#[tokio::test]
async fn osv_refuses_vulnerable_tarball_from_cache() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"cached vulnerable tarball";
    let packument_mock = mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", bytes).await;
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let warming_app = router(config_for(&upstream.url(), cache_dir.clone()));

    let warmed = warming_app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(warmed.status(), StatusCode::OK);
    assert_eq!(body_bytes(warmed.into_body()).await, bytes);

    let osv = osv_database("foo", &["1.0.0"]);
    let mut config = config_for(&upstream.url(), cache_dir);
    config.resolver.enabled = false;
    enable_osv(&mut config, osv.path());
    let screened_app = router(config);

    let response = screened_app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    packument_mock.assert_async().await;
    mock.assert_async().await;
}

#[tokio::test]
async fn osv_refuses_vulnerable_cached_tarball_under_noncanonical_name() {
    // A cache hit must still be OSV-screened on the tarball's *resolved*
    // version, not just the filename-carried one. Here version 1.0.0 declares
    // a non-canonical tarball basename `foo-0.0.1.tgz`, so the request filename
    // resolves to 1.0.0 — and a cached tarball for a now-blocked 1.0.0 must not
    // slip past on the filename's unblocked "0.0.1".
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"cached vulnerable tarball";
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/foo/-/foo-0.0.1.tgz", upstream.url()),
                    "integrity": sha512_integrity(bytes),
                },
            },
        },
    });
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-0.0.1.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let warming_app = router(config_for(&upstream.url(), cache_dir.clone()));
    let warmed = warming_app
        .oneshot(Request::get("/foo/-/foo-0.0.1.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(warmed.status(), StatusCode::OK);
    assert_eq!(body_bytes(warmed.into_body()).await, bytes);

    // The warm-up must have written the tarball to the proxy cache, so the
    // screened request below genuinely exercises the cache-hit path rather than
    // silently falling back to a miss that would be refused anyway.
    let cached_tarball = public_cache_pkg(&cache_dir, "foo").join("foo-0.0.1.tgz");
    assert!(
        cached_tarball.is_file(),
        "warm-up must populate the proxy cache at {cached_tarball:?}",
    );

    // Block the resolved version 1.0.0; the filename carries the unblocked
    // 0.0.1, so only a resolved-version screen on the cache hit can refuse.
    let osv = osv_database("foo", &["1.0.0"]);
    let mut config = config_for(&upstream.url(), cache_dir);
    config.resolver.enabled = false;
    enable_osv(&mut config, osv.path());
    let screened_app = router(config);

    let response = screened_app
        .oneshot(Request::get("/foo/-/foo-0.0.1.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn upstream_404_is_propagated() {
    let mut upstream = mockito::Server::new_async().await;
    let _mock = upstream
        .mock("GET", "/missing")
        .with_status(404)
        .with_body("Not Found")
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response =
        app.oneshot(Request::get("/missing").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn upstream_5xx_maps_to_bad_gateway() {
    let mut upstream = mockito::Server::new_async().await;
    let _mock =
        upstream.mock("GET", "/broken").with_status(500).with_body("kaboom").create_async().await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app.oneshot(Request::get("/broken").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn unreachable_upstream_maps_to_service_unavailable() {
    // Port 0 is never a listening port, so connecting to it is refused
    // outright — exercising the `is_connect()` branch of the status
    // mapping without depending on DNS. Probing for a free ephemeral
    // port instead would race: the suite's mockito servers bind ephemeral
    // ports throughout the run and can claim the probed one before the
    // request goes out, turning the refusal into a success.
    let dead_upstream = "http://127.0.0.1:0".to_string();

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&dead_upstream, tmp.path().to_path_buf()));

    let response = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn tarball_filename_for_other_package_is_rejected() {
    // A non-canonical filename is admitted only when the package's own
    // packument declares that basename in some version's `dist.tarball` (the
    // preserved-upstream-basename case). `foo`'s packument declares only
    // `foo-1.0.0.tgz`, so another package's filename is a definitive 404 —
    // never fetched, never cached under `foo`.
    let mut upstream = mockito::Server::new_async().await;
    upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(foo_packument(&upstream.url()).to_string())
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app
        .clone()
        .oneshot(Request::get("/foo/-/bar-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // A filename that is not even a safe path segment stays an early 400,
    // before any packument or tarball I/O.
    let unsafe_name = app
        .oneshot(Request::get("/foo/-/..%5Cescape.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(unsafe_name.status(), StatusCode::BAD_REQUEST);
}

/// An upstream may host a version under a basename that doesn't follow the
/// canonical `<name>-<version>.tgz` shape (e.g. esprima-fb's zero-padded
/// `3001.0001.0000-dev-harmony-fb`). The rewrite preserves that basename in
/// the served packument, so the serve path must accept it back and bind it
/// to the declaring version's integrity — otherwise the version is
/// un-fetchable through the very URL this server advertised.
#[tokio::test]
async fn non_canonical_upstream_tarball_basename_is_served() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"exotic-basename-tarball";
    let exotic = "legacy_foo_build.tgz";
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "3001.1.0-exotic" },
        "versions": { "3001.1.0-exotic": {
            "name": "foo",
            "version": "3001.1.0-exotic",
            "dist": {
                "tarball": format!("{}/foo/-/{exotic}", upstream.url()),
                "integrity": sha512_integrity(bytes),
            },
        } },
    });
    upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", format!("/foo/-/{exotic}").as_str())
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    // The served packument advertises the preserved basename on this server.
    let served =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    let doc = body_json(served.into_body()).await;
    let advertised = doc["versions"]["3001.1.0-exotic"]["dist"]["tarball"].as_str().unwrap();
    assert!(advertised.ends_with(&format!("/foo/-/{exotic}")), "got {advertised}");

    // And fetching that URL back serves the verified bytes.
    let response = app
        .oneshot(Request::get(format!("/foo/-/{exotic}").as_str()).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn scoped_tarball_is_proxied() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"scoped-tarball-bytes";
    let _packument_mock =
        mock_packument_for_tarball(&mut upstream, "@types/node", "20.0.0", bytes).await;
    let mock = upstream
        .mock("GET", "/@types/node/-/node-20.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app
        .oneshot(Request::get("/@types/node/-/node-20.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
    mock.assert_async().await;
}

#[tokio::test]
async fn scoped_tarball_filename_is_canonicalized_before_fetch_and_cache() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"scoped-tarball-full-name";
    let packument_mock =
        mock_packument_for_tarball(&mut upstream, "@types/node", "20.0.0", bytes).await;
    let mock = upstream
        .mock("GET", "/@types/node/-/node-20.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), storage.clone()));

    let noncanonical = app
        .clone()
        .oneshot(
            Request::get("/@types/node/-/%40types%2Fnode-20.0.0.tgz").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(noncanonical.status(), StatusCode::OK);
    assert_eq!(body_bytes(noncanonical.into_body()).await, bytes);

    let canonical = app
        .oneshot(Request::get("/@types/node/-/node-20.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(canonical.status(), StatusCode::OK);
    assert_eq!(body_bytes(canonical.into_body()).await, bytes);
    assert!(public_cache_pkg(&storage, "@types/node").join("node-20.0.0.tgz").exists());
    assert!(!public_cache_pkg(&storage, "@types/node").join("@types").exists());
    packument_mock.assert_async().await;
    mock.assert_async().await;
}
