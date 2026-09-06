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

#[tokio::test]
async fn fetch_packument_forwards_configured_headers() {
    let mut server = mockito::Server::new_async().await;
    // The mock only matches when both headers are present, so an
    // `Ok` outcome proves they rode along on the request.
    let mock = server
        .mock("GET", "/foo")
        .match_header("authorization", "Bearer secret-token")
        .match_header("x-org", "acme")
        .with_status(200)
        .with_body(json!({ "name": "foo" }).to_string())
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(server.url(), auth_and_custom_headers());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let outcome = upstream.fetch_packument(&name, &CacheValidators::default()).await.unwrap();

    assert!(matches!(outcome, PackumentFetch::Modified(_)));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_tarball_response_forwards_configured_headers() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .match_header("authorization", "Bearer secret-token")
        .match_header("x-org", "acme")
        .with_status(200)
        .with_body("tarball-bytes")
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(server.url(), auth_and_custom_headers());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let outcome = upstream.fetch_tarball_response(&name, "foo-1.0.0.tgz").await.unwrap();

    assert!(matches!(outcome, FetchOutcome::Ok(_)));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_revision_tarball_rejects_redirects_and_forwards_headers() {
    let mut server = mockito::Server::new_async().await;
    let redirect = server
        .mock("GET", "/-/tarballs/sha512/digest")
        .match_header("authorization", "Bearer secret-token")
        .match_header("x-org", "acme")
        .with_status(302)
        .with_header("location", "/redirected.tgz")
        .with_body("x".repeat(UPSTREAM_ERROR_BODY_LIMIT + 1))
        .expect(1)
        .create_async()
        .await;
    let redirected = server.mock("GET", "/redirected.tgz").expect(0).create_async().await;

    let upstream = upstream(server.url(), auth_and_custom_headers());
    let result = upstream.fetch_revision_tarball_response("digest").await;

    assert!(matches!(
        result,
        Err(RegistryError::UpstreamStatus { status: 302, body, .. })
            if body == format!("{} (response body truncated)", "x".repeat(UPSTREAM_ERROR_BODY_LIMIT))
    ));
    redirect.assert_async().await;
    redirected.assert_async().await;
}

#[tokio::test]
async fn discovery_rejects_redirects_before_configured_headers_reach_the_target() {
    let mut target = mockito::Server::new_async().await;
    let redirected = target
        .mock("GET", "/-/v1/search")
        .match_header("authorization", mockito::Matcher::Missing)
        .expect(0)
        .create_async()
        .await;
    let mut source = mockito::Server::new_async().await;
    let redirect = source
        .mock("GET", "/-/v1/search")
        .match_query("text=foo")
        .match_header("authorization", "Bearer secret-token")
        .with_status(302)
        .with_header("location", &format!("{}/-/v1/search", target.url()))
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(source.url(), auth_and_custom_headers());
    let result = upstream.fetch_search("text=foo").await;

    assert!(
        matches!(result, Err(RegistryError::UpstreamStatus { status: 302, .. })),
        "expected the first redirect response, got {result:?}",
    );
    redirect.assert_async().await;
    redirected.assert_async().await;
}

#[test]
fn configured_headers_require_a_secure_same_origin_destination() {
    for base in ["http://registry.example", "https://registry.example", "http://127.0.0.1"] {
        let upstream = upstream(base.to_string(), auth_and_custom_headers());
        assert_eq!(
            upstream.request_headers(&format!("{base}/metadata")).is_empty(),
            base == "http://registry.example",
        );
        assert!(upstream.request_headers("https://other.example/metadata").is_empty());
    }
}

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

#[tokio::test]
async fn fetch_packument_sends_no_authorization_when_headers_empty() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/foo")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(200)
        .with_body(json!({ "name": "foo" }).to_string())
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(server.url(), HeaderMap::new());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let outcome = upstream.fetch_packument(&name, &CacheValidators::default()).await.unwrap();

    assert!(matches!(outcome, PackumentFetch::Modified(_)));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_packument_modified_carries_the_body() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(json!({ "name": "foo" }).to_string())
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(server.url(), HeaderMap::new());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let outcome = upstream.fetch_packument(&name, &CacheValidators::default()).await.unwrap();

    let PackumentFetch::Modified(fetched) = outcome else { panic!("expected a body") };
    assert!(!fetched.bytes.is_empty(), "a modified fetch carries the packument body");
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_packument_replays_validators_and_handles_304() {
    let mut server = mockito::Server::new_async().await;
    // The mock only matches when both conditional headers are present,
    // so a `NotModified` outcome proves they rode along on the request.
    let mock = server
        .mock("GET", "/foo")
        .match_header("if-none-match", r#""abc123""#)
        .match_header("if-modified-since", "Wed, 21 Oct 2015 07:28:00 GMT")
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(server.url(), HeaderMap::new());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let validators = CacheValidators {
        etag: Some(r#""abc123""#.to_string()),
        last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
    };
    let outcome = upstream.fetch_packument(&name, &validators).await.unwrap();

    assert!(matches!(outcome, PackumentFetch::NotModified));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_packument_304_without_validators_is_an_error() {
    let mut server = mockito::Server::new_async().await;
    // No conditional header is sent (empty validators), so a `304` here is
    // a misbehaving upstream — there's no body and nothing to revalidate
    // against. It must surface as an error, not a `NotModified` that the
    // caller could mistake for "keep serving the cache".
    let mock = server
        .mock("GET", "/foo")
        .match_header("if-none-match", mockito::Matcher::Missing)
        .match_header("if-modified-since", mockito::Matcher::Missing)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(server.url(), HeaderMap::new());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let result = upstream.fetch_packument(&name, &CacheValidators::default()).await;

    assert!(result.is_err(), "an unconditional 304 must not be treated as NotModified");
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_packument_maps_404_to_not_found() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/foo").with_status(404).expect(1).create_async().await;

    let upstream = upstream(server.url(), HeaderMap::new());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let outcome = upstream.fetch_packument(&name, &CacheValidators::default()).await.unwrap();

    assert!(matches!(outcome, PackumentFetch::NotFound));
    mock.assert_async().await;
}

#[test]
fn rewrites_npm_form_tarball() {
    let mut doc = json!({
        "name": "foo",
        "versions": {
            "1.0.0": {
                "dist": {
                    "tarball": "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz",
                    "shasum": "abc"
                }
            }
        }
    });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:4873");
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        "http://127.0.0.1:4873/foo/-/foo-1.0.0.tgz",
    );
    assert_eq!(doc["versions"]["1.0.0"]["dist"]["shasum"], "abc");
}

#[test]
fn preserves_valid_upstream_revision_route_on_the_public_registry() {
    let current_digest = "A".repeat(86);
    let original_digest = "Q".repeat(86);
    let mut doc = json!({
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("https://upstream.test/npm/-/tarballs/sha512/{current_digest}"),
                    "integrity": format!("sha512-{current_digest}=="),
                    "revision": 2,
                    "revisions": [{
                        "revision": 0,
                        "integrity": format!("sha512-{original_digest}=="),
                        "tarball": format!("https://upstream.test/npm/-/tarballs/sha512/{original_digest}"),
                        "manifest": { "dependencies": { "bar": "1" } },
                    }, {
                        "revision": 2,
                        "integrity": format!("sha512-{current_digest}=="),
                        "tarball": format!("https://upstream.test/npm/-/tarballs/sha512/{current_digest}"),
                        "manifest": {},
                    }],
                }
            }
        }
    });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();

    rewrite_upstream_tarball_urls(
        &mut doc,
        &name,
        "https://upstream.test/npm/",
        "http://pnpr.test/~corp/",
    );

    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        format!("http://pnpr.test/~corp/-/tarballs/sha512/{current_digest}"),
    );
    assert_eq!(doc["versions"]["1.0.0"]["dist"]["revision"], 2);
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["revisions"][0]["tarball"],
        format!("http://pnpr.test/~corp/-/tarballs/sha512/{original_digest}"),
    );
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["revisions"][1]["tarball"],
        format!("http://pnpr.test/~corp/-/tarballs/sha512/{current_digest}"),
    );
}

#[test]
fn drops_invalid_upstream_revision_history_entries() {
    let digest = "A".repeat(86);
    let mut doc = json!({
        "version": "1.0.0",
        "dist": {
            "tarball": format!("https://upstream.test/-/tarballs/sha512/{digest}"),
            "integrity": format!("sha512-{digest}=="),
            "revision": 2,
            "revisions": [{
                "revision": 1,
                "integrity": format!("sha512-{digest}=="),
                "tarball": format!("https://other.test/-/tarballs/sha512/{digest}"),
                "manifest": {},
            }],
        },
    });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();

    rewrite_upstream_tarball_urls(&mut doc, &name, "https://upstream.test/", "http://pnpr.test/");

    assert_eq!(doc["dist"]["revisions"], json!([]));
}

#[test]
fn does_not_preserve_an_invalid_upstream_revision_route() {
    let digest = "A".repeat(86);
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    for (revision, tarball) in [
        (json!(0), format!("https://upstream.test/-/tarballs/sha512/{digest}")),
        (json!(2), format!("https://other.test/-/tarballs/sha512/{digest}")),
    ] {
        let mut doc = json!({
            "version": "1.0.0",
            "dist": {
                "tarball": tarball,
                "integrity": format!("sha512-{digest}=="),
                "revision": revision,
            }
        });

        rewrite_upstream_tarball_urls(
            &mut doc,
            &name,
            "https://upstream.test/",
            "http://pnpr.test/~corp/",
        );

        assert_eq!(doc["dist"]["tarball"], format!("http://pnpr.test/~corp/foo/-/{digest}"));
        assert!(doc["dist"].get("revision").is_none());
    }
}

#[test]
fn rewrites_verdaccio_form_tarball_for_scoped() {
    // Verdaccio publishes scoped tarball URLs like
    // `/@scope/name/-/@scope/name-1.0.0.tgz` — the scope is
    // present twice. We only care about the basename.
    let mut doc = json!({
        "versions": {
            "1.0.0": {
                "dist": {
                    "tarball": "http://localhost:4873/@foo/no-deps/-/@foo/no-deps-1.0.0.tgz"
                }
            }
        }
    });
    let name =
        CanonicalPackageName::parse("@foo/no-deps", pnpr_package_name::Ecosystem::Npm).unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:9999");
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        "http://127.0.0.1:9999/@foo/no-deps/-/no-deps-1.0.0.tgz",
    );
}

#[test]
fn preserves_non_canonical_tarball_basename() {
    // A version whose tarball filename does not follow the
    // `<name>-<version>.tgz` convention (here esprima-fb's zero-padded
    // name for version `3001.1.0-dev-harmony-fb`) keeps its basename, so
    // the client records and fetches back the path the upstream hosts.
    let mut doc = json!({
        "versions": {
            "3001.1.0-dev-harmony-fb": {
                "dist": {
                    "tarball": "https://registry.npmjs.org/esprima-fb/-/esprima-fb-3001.0001.0000-dev-harmony-fb.tgz"
                }
            }
        }
    });
    let name =
        CanonicalPackageName::parse("esprima-fb", pnpr_package_name::Ecosystem::Npm).unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:9999");
    assert_eq!(
        doc["versions"]["3001.1.0-dev-harmony-fb"]["dist"]["tarball"],
        "http://127.0.0.1:9999/esprima-fb/-/esprima-fb-3001.0001.0000-dev-harmony-fb.tgz",
    );
}

#[test]
fn rewrites_query_bearing_tarball_to_its_path_basename() {
    // A signed/query-bearing upstream URL must rewrite to the path basename
    // alone: the query is not part of the route a client later requests, so
    // keeping it would desync the public URL from the serve-time match.
    let mut doc = json!({
        "versions": {
            "1.0.0": {
                "dist": { "tarball": "https://cdn.example.com/foo/-/foo-1.0.0.tgz?sig=abc&exp=123" }
            }
        }
    });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:9999");
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        "http://127.0.0.1:9999/foo/-/foo-1.0.0.tgz",
    );
}

#[test]
fn rewrites_basenameless_tarball_url_to_pnpr_route() {
    // A dist.tarball with no usable basename (here a trailing slash) must
    // never be passed through to the client; it falls back to the
    // version-derived pnpr route so integrity/OSV stay enforced here and the
    // client is never directed at the upstream host.
    let mut doc = json!({
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "dist": { "tarball": "https://evil.example.com/somewhere/" }
            }
        }
    });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:9999");
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        "http://127.0.0.1:9999/foo/-/foo-1.0.0.tgz",
    );
}

#[test]
fn tarball_basename_strips_query_and_fragment() {
    let base = "foo-1.0.0.tgz";
    assert_eq!(tarball_basename("https://r/foo/-/foo-1.0.0.tgz"), Some(base));
    assert_eq!(tarball_basename("https://r/foo/-/foo-1.0.0.tgz?sig=x"), Some(base));
    assert_eq!(tarball_basename("https://r/foo/-/foo-1.0.0.tgz#frag"), Some(base));
    assert_eq!(tarball_basename("foo-1.0.0.tgz"), Some(base));
    assert_eq!(tarball_basename("https://r/foo/"), None);
}

#[test]
fn handles_packument_without_versions() {
    let mut doc = json!({ "name": "foo" });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:4873");
    assert_eq!(doc, json!({ "name": "foo" }));
}

#[test]
fn extracts_version_by_dist_tag() {
    let doc = json!({
        "name": "@foo/no-deps",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@foo/no-deps",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://localhost:4873/@foo/no-deps/-/@foo/no-deps-1.0.0.tgz",
                    "shasum": "abc"
                }
            }
        }
    });
    let name =
        CanonicalPackageName::parse("@foo/no-deps", pnpr_package_name::Ecosystem::Npm).unwrap();
    let manifest = extract_version_manifest(&doc, &name, "latest", "http://reg").unwrap();
    assert_eq!(manifest["version"], "1.0.0");
    assert_eq!(manifest["dist"]["tarball"], "http://reg/@foo/no-deps/-/no-deps-1.0.0.tgz");
    assert_eq!(manifest["dist"]["shasum"], "abc");
}

#[test]
fn extracts_version_by_literal_version() {
    let doc = json!({
        "name": "foo",
        "versions": { "2.0.0": { "version": "2.0.0", "dist": { "tarball": "x/foo-2.0.0.tgz" } } }
    });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let manifest = extract_version_manifest(&doc, &name, "2.0.0", "http://reg").unwrap();
    assert_eq!(manifest["version"], "2.0.0");
    assert_eq!(manifest["dist"]["tarball"], "http://reg/foo/-/foo-2.0.0.tgz");
}

#[test]
fn extract_returns_none_for_unknown_version() {
    let doc = json!({
        "versions": { "1.0.0": { "dist": { "tarball": "x/foo-1.0.0.tgz" } } }
    });
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    assert!(extract_version_manifest(&doc, &name, "9.9.9", "http://reg").is_none());
    assert!(extract_version_manifest(&doc, &name, "latest", "http://reg").is_none());
}

#[test]
fn abbreviation_drops_fields_the_resolver_ignores() {
    let doc = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "modified": "2020-01-01T00:00:00.000Z", "1.0.0": "2019-01-01T00:00:00.000Z" },
        "_id": "foo",
        "_rev": "1-abc",
        "readme": "# foo\nlots of prose",
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dependencies": { "bar": "^1.0.0" },
                "devDependencies": { "jest": "^29.0.0" },
                "peerDependencies": { "react": "*" },
                "os": ["linux"],
                "cpu": ["x64"],
                "libc": ["glibc"],
                "funding": { "url": "https://example.com" },
                "acceptDependencies": { "bar": "^1.0.0" },
                "_hasShrinkwrap": false,
                "hasInstallScript": true,
                "dist": {
                    "tarball": "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz",
                    "integrity": "sha512-abc",
                    "shasum": "deadbeef",
                    "fileCount": 12,
                    "unpackedSize": 34567,
                    "signatures": [{ "keyid": "SHA256:xyz", "sig": "base64sig" }],
                    "npm-signature": "-----BEGIN PGP SIGNATURE-----"
                }
            }
        }
    });

    let out = abbreviate_packument(&doc, now());

    // Top-level: prose and registry bookkeeping gone, `modified`
    // synthesized from `time.modified`, `time` retained. Both `time`
    // entries predate the week-old horizon, so they coarsen to bare
    // dates.
    assert!(out.get("readme").is_none());
    assert!(out.get("readmeFilename").is_none());
    assert!(out.get("_id").is_none());
    assert!(out.get("_rev").is_none());
    assert_eq!(out["modified"], "2020-01-01");
    assert_eq!(out["time"]["modified"], "2020-01-01");
    assert_eq!(out["time"]["1.0.0"], "2019-01-01");

    let version = &out["versions"]["1.0.0"];
    // Resolver-relevant fields kept.
    assert_eq!(version["name"], "foo");
    assert_eq!(version["dependencies"]["bar"], "^1.0.0");
    assert_eq!(version["peerDependencies"]["react"], "*");
    assert_eq!(version["hasInstallScript"], true);
    // Platform-filtering fields kept for optional-dep selection (`#9950`).
    assert_eq!(version["os"][0], "linux");
    assert_eq!(version["cpu"][0], "x64");
    assert_eq!(version["libc"][0], "glibc");
    // Ignored fields dropped.
    assert!(version.get("devDependencies").is_none());
    assert!(version.get("funding").is_none());
    assert!(version.get("acceptDependencies").is_none());
    assert!(version.get("_hasShrinkwrap").is_none());
    // `shasum` dropped because `integrity` is present.
    assert_eq!(version["dist"]["integrity"], "sha512-abc");
    assert!(version["dist"].get("shasum").is_none());
    // Legacy PGP signature dropped; ECDSA registry signatures kept.
    assert!(version["dist"].get("npm-signature").is_none());
    assert_eq!(version["dist"]["signatures"][0]["keyid"], "SHA256:xyz");
    // Size hints kept: pacquet reads both for decompression
    // preallocation and download scheduling.
    assert_eq!(version["dist"]["fileCount"], 12);
    assert_eq!(version["dist"]["unpackedSize"], 34567);
}

#[test]
fn abbreviation_keeps_shasum_when_integrity_absent() {
    let doc = json!({
        "name": "legacy",
        "versions": {
            "0.0.1": {
                "name": "legacy",
                "version": "0.0.1",
                "dist": {
                    "tarball": "https://registry.npmjs.org/legacy/-/legacy-0.0.1.tgz",
                    "shasum": "deadbeef"
                }
            }
        }
    });

    let out = abbreviate_packument(&doc, now());

    let dist = &out["versions"]["0.0.1"]["dist"];
    assert!(dist.get("integrity").is_none());
    assert_eq!(dist["shasum"], "deadbeef");
}

#[test]
fn abbreviation_keeps_shasum_when_integrity_is_empty_or_non_string() {
    // pnpm's `getIntegrity` falls back to `shasum` unless `integrity`
    // is a truthy (non-empty) string, so an empty or malformed
    // `integrity` must not strip the sha1 fallback.
    let doc = json!({
        "name": "weird",
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "dist": { "tarball": "x/weird-1.0.0.tgz", "integrity": "", "shasum": "deadbeef" }
            },
            "2.0.0": {
                "version": "2.0.0",
                "dist": { "tarball": "x/weird-2.0.0.tgz", "integrity": false, "shasum": "cafe" }
            }
        }
    });

    let out = abbreviate_packument(&doc, now());

    assert_eq!(out["versions"]["1.0.0"]["dist"]["shasum"], "deadbeef");
    assert_eq!(out["versions"]["2.0.0"]["dist"]["shasum"], "cafe");
}

#[test]
fn coarsens_time_entries_by_age() {
    // `now()` is 2024-03-20T12:00Z; the horizon is one week earlier
    // (2024-03-13T12:00Z). Values are rounded *up* so the coarsened
    // timestamp never predates the real publish time.
    let doc = json!({
        "name": "foo",
        "time": {
            // Older than a week: rounded up to the next day...
            "modified": "2024-03-01T08:15:30.500Z",
            "1.0.0": "2023-12-25T23:59:59.000Z",
            // ...unless already exactly midnight, which stays put.
            "2.0.0": "2024-01-10T00:00:00.000Z",
            // Within the last week: rounded up to the next minute...
            "1.1.0": "2024-03-19T08:30:45.123Z",
            // ...unless already on a minute boundary.
            "1.2.0": "2024-03-18T06:15:00.000Z",
            // The reserved `unpublished` object passes through verbatim.
            "unpublished": { "time": "2024-03-18T00:00:00.000Z", "versions": ["0.9.0"] },
            // An unparsable value is left untouched.
            "0.0.1": "not a date"
        },
        "versions": {
            "1.0.0": { "version": "1.0.0", "dist": { "tarball": "x/foo-1.0.0.tgz" } }
        }
    });

    let out = abbreviate_packument(&doc, now());
    let time = &out["time"];

    assert_eq!(time["modified"], "2024-03-02");
    assert_eq!(time["1.0.0"], "2023-12-26");
    assert_eq!(time["2.0.0"], "2024-01-10");
    assert_eq!(time["1.1.0"], "2024-03-19T08:31Z");
    assert_eq!(time["1.2.0"], "2024-03-18T06:15Z");
    assert_eq!(time["unpublished"]["versions"][0], "0.9.0");
    assert_eq!(time["0.0.1"], "not a date");
    // Synthesized top-level `modified` mirrors the coarsened entry.
    assert_eq!(out["modified"], "2024-03-02");
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

#[tokio::test]
async fn fetch_document_forwards_headers_and_accept_and_reports_the_final_url() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/simple/requests/")
        .match_header("authorization", "Bearer secret-token")
        .match_header("accept", "application/vnd.pypi.simple.v1+json")
        .with_body(r#"{"name":"requests"}"#)
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(format!("{}/simple/", server.url()), auth_and_custom_headers());
    let outcome = upstream
        .fetch_document("requests/", Some("application/vnd.pypi.simple.v1+json"), 1024)
        .await
        .unwrap();
    let FetchOutcome::Ok(document) = outcome else { panic!("expected a document") };
    assert_eq!(document.bytes, br#"{"name":"requests"}"#);
    assert_eq!(document.url, format!("{}/simple/requests/", server.url()));
    mock.assert_async().await;

    let missing = server.mock("GET", "/simple/nope/").with_status(404).create_async().await;
    let outcome = upstream.fetch_document("nope/", None, 1024).await.unwrap();
    assert!(matches!(outcome, FetchOutcome::NotFound));
    missing.assert_async().await;
}

#[tokio::test]
async fn fetch_document_rejects_a_body_over_the_limit() {
    let mut server = mockito::Server::new_async().await;
    let mock =
        server.mock("GET", "/se/rd/serde").with_body("x".repeat(64)).expect(2).create_async().await;
    let upstream = breaking_upstream(server.url(), 1);
    for _ in 0..2 {
        let err = upstream.fetch_document("se/rd/serde", None, 16).await.unwrap_err();
        assert!(matches!(err, RegistryError::UpstreamResponse { .. }), "{err:?}");
    }
    mock.assert_async().await;
}

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
    let response =
        upstream.fetch_artifact_response(&format!("{}/dl/serde/1.0.0", index.url())).await.unwrap();
    let FetchOutcome::Ok(response) = response else { panic!("expected a response") };
    assert_eq!(response.bytes().await.unwrap(), "crate bytes");
    let response =
        upstream.fetch_artifact_response(&format!("{}/packages/x.whl", files.url())).await.unwrap();
    let FetchOutcome::Ok(response) = response else { panic!("expected a response") };
    assert_eq!(response.bytes().await.unwrap(), "wheel bytes");
    same_origin.assert_async().await;
    other_origin.assert_async().await;
}

#[tokio::test]
async fn configured_headers_are_removed_on_metadata_redirects() {
    let mut source = mockito::Server::new_async().await;
    let mut target = mockito::Server::new_async().await;
    let target_mock = target
        .mock("GET", "/leak")
        .match_header("authorization", mockito::Matcher::Missing)
        .match_header("x-org", mockito::Matcher::Missing)
        .with_body("metadata")
        .expect(1)
        .create_async()
        .await;
    let redirect = source
        .mock("GET", "/metadata")
        .with_status(302)
        .with_header("location", &format!("{}/leak", target.url()))
        .expect(1)
        .create_async()
        .await;
    let upstream = upstream(source.url(), auth_and_custom_headers());
    assert!(matches!(
        upstream.fetch_document("metadata", None, 1024).await.unwrap(),
        FetchOutcome::Ok(_)
    ));
    target_mock.assert_async().await;
    redirect.assert_async().await;
}

#[tokio::test]
async fn artifact_fetch_guard_rejects_initial_urls_and_redirects() {
    let mut source = mockito::Server::new_async().await;
    let mut target = mockito::Server::new_async().await;
    let target_mock = target.mock("GET", "/artifact").expect(0).create_async().await;
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
    assert!(upstream.fetch_artifact_response(&format!("{}/artifact", target.url())).await.is_err());
    assert!(upstream.fetch_artifact_response(&format!("{}/artifact", source.url())).await.is_err());
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
async fn redirect_chain_shares_one_timeout() {
    assert_redirect_timeout(false).await;
}

#[tokio::test]
async fn redirected_artifact_body_uses_remaining_timeout() {
    assert_redirect_timeout(true).await;
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
    upstream.client = std::sync::Arc::new(
        pnpm_network::ThrottledClient::new_for_installs().with_max_sockets_per_host(Some(1)),
    );
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    for mode in 0..3 {
        let outcome = match mode {
            0 => upstream.fetch_artifact_response(&server.url()).await,
            1 => upstream.fetch_tarball_response(&name, "foo.tgz").await,
            _ => upstream.fetch_revision_tarball_response("digest").await,
        };
        let FetchOutcome::Ok(response) = outcome.unwrap() else {
            panic!("expected artifact response")
        };
        assert!(
            tokio::time::timeout(
                Duration::from_millis(20),
                upstream.client.acquire_for_url(&server.url())
            )
            .await
            .is_err(),
        );
        drop(response);
        let guard = tokio::time::timeout(
            Duration::from_secs(1),
            upstream.client.acquire_for_url(&server.url()),
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
    config.timeout = Duration::from_millis(250);
    let mut upstream = Upstream::new("test", &config);
    upstream.client = std::sync::Arc::new(
        pnpm_network::ThrottledClient::new_for_installs().with_max_sockets_per_host(Some(1)),
    );
    let held = upstream.client.acquire_for_url(&url).await;
    let fetch = upstream.fetch_revision_tarball_response("digest");
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
