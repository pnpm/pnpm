use super::{
    CacheValidators, FetchOutcome, PackumentFetch, Upstream, abbreviate_packument,
    extract_version_manifest, rewrite_tarball_urls,
};
use crate::package_name::PackageName;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde_json::json;

/// Fixed "current time" for abbreviation tests so the `time`-map
/// coarsening (which buckets entries by age) is deterministic.
fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 3, 20, 12, 0, 0).unwrap()
}

/// Build a header map carrying a bearer `Authorization` plus one
/// custom header — the resolved per-uplink set an [`Upstream`] is
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

    let upstream = Upstream::new(server.url(), auth_and_custom_headers());
    let name = PackageName::parse("foo").unwrap();
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

    let upstream = Upstream::new(server.url(), auth_and_custom_headers());
    let name = PackageName::parse("foo").unwrap();
    let outcome = upstream.fetch_tarball_response(&name, "foo-1.0.0.tgz").await.unwrap();

    assert!(matches!(outcome, FetchOutcome::Ok(_)));
    mock.assert_async().await;
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

    let upstream = Upstream::new(server.url(), HeaderMap::new());
    let name = PackageName::parse("foo").unwrap();
    let outcome = upstream.fetch_packument(&name, &CacheValidators::default()).await.unwrap();

    assert!(matches!(outcome, PackumentFetch::Modified(_)));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_packument_captures_validators_from_response() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("etag", r#""abc123""#)
        .with_header("last-modified", "Wed, 21 Oct 2015 07:28:00 GMT")
        .with_body(json!({ "name": "foo" }).to_string())
        .expect(1)
        .create_async()
        .await;

    let upstream = Upstream::new(server.url(), HeaderMap::new());
    let name = PackageName::parse("foo").unwrap();
    let outcome = upstream.fetch_packument(&name, &CacheValidators::default()).await.unwrap();

    let PackumentFetch::Modified(fetched) = outcome else { panic!("expected a body") };
    assert_eq!(fetched.validators.etag.as_deref(), Some(r#""abc123""#));
    assert_eq!(fetched.validators.last_modified.as_deref(), Some("Wed, 21 Oct 2015 07:28:00 GMT"));
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

    let upstream = Upstream::new(server.url(), HeaderMap::new());
    let name = PackageName::parse("foo").unwrap();
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

    let upstream = Upstream::new(server.url(), HeaderMap::new());
    let name = PackageName::parse("foo").unwrap();
    let result = upstream.fetch_packument(&name, &CacheValidators::default()).await;

    assert!(result.is_err(), "an unconditional 304 must not be treated as NotModified");
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_packument_maps_404_to_not_found() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/foo").with_status(404).expect(1).create_async().await;

    let upstream = Upstream::new(server.url(), HeaderMap::new());
    let name = PackageName::parse("foo").unwrap();
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
    let name = PackageName::parse("foo").unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:4873");
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        "http://127.0.0.1:4873/foo/-/foo-1.0.0.tgz",
    );
    assert_eq!(doc["versions"]["1.0.0"]["dist"]["shasum"], "abc");
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
    let name = PackageName::parse("@foo/no-deps").unwrap();
    rewrite_tarball_urls(&mut doc, &name, "http://127.0.0.1:9999");
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        "http://127.0.0.1:9999/@foo/no-deps/-/no-deps-1.0.0.tgz",
    );
}

#[test]
fn handles_packument_without_versions() {
    let mut doc = json!({ "name": "foo" });
    let name = PackageName::parse("foo").unwrap();
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
    let name = PackageName::parse("@foo/no-deps").unwrap();
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
    let name = PackageName::parse("foo").unwrap();
    let manifest = extract_version_manifest(&doc, &name, "2.0.0", "http://reg").unwrap();
    assert_eq!(manifest["version"], "2.0.0");
    assert_eq!(manifest["dist"]["tarball"], "http://reg/foo/-/foo-2.0.0.tgz");
}

#[test]
fn extract_returns_none_for_unknown_version() {
    let doc = json!({
        "versions": { "1.0.0": { "dist": { "tarball": "x/foo-1.0.0.tgz" } } }
    });
    let name = PackageName::parse("foo").unwrap();
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
