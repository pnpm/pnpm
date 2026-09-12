use super::{
    FAKE_INTEGRITY, HashMap, LockfileResolution, PkgName, REVISION_ONE_DIGEST, REVISION_TWO_DIGEST,
    RegistryResolution, ResolutionVerification, TarballResolution, TarballRevision, TrustPolicy,
    Utc, abbreviated_packument_json, assert_eq, create_npm_resolution_verifier,
    create_package_version_policy, ctx, default_opts, fake_integrity, min_age_packument_json,
    now_at, registry_resolution, revision_integrity, tarball_resolution,
};
use pnpm_resolving_resolver_base::ResolutionVerifier;

#[test]
fn does_not_verify_a_revision_without_an_active_policy() {
    let verifier = create_npm_resolution_verifier(default_opts("https://registry.example/"));
    let resolution = LockfileResolution::Registry(RegistryResolution {
        integrity: revision_integrity(REVISION_ONE_DIGEST),
        revision: Some(TarballRevision::try_from(1).unwrap()),
    });
    let name = "revision-pkg".parse::<PkgName>().unwrap();
    assert!(!verifier.might_verify(&resolution, ctx(&name, "1.0.0")));
}

#[tokio::test]
async fn rejects_an_explicit_zero_current_revision() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let packument = serde_json::json!({
        "name": "revision-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "revision-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": revision_integrity(REVISION_ONE_DIGEST).to_string(),
                    "tarball": format!("{registry}revision-pkg/-/revision-pkg-1.0.0.tgz"),
                    "revision": 0,
                }
            }
        },
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" }
    });
    server
        .mock("GET", "/revision-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(1);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = tarball_resolution(
        &format!("{registry}revision-pkg/-/revision-pkg-1.0.0.tgz"),
        Some(fake_integrity()),
    );
    let name = "revision-pkg".parse::<PkgName>().unwrap();
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TARBALL_REVISION_MISMATCH");
}

#[tokio::test]
async fn rejects_a_non_numeric_current_revision() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let packument = serde_json::json!({
        "name": "revision-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "revision-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "tarball": format!("{registry}revision-pkg/-/revision-pkg-1.0.0.tgz"),
                    "revision": "1",
                }
            }
        },
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" }
    });
    server
        .mock("GET", "/revision-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(1);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = tarball_resolution(
        &format!("{registry}revision-pkg/-/revision-pkg-1.0.0.tgz"),
        Some(fake_integrity()),
    );
    let name = "revision-pkg".parse::<PkgName>().unwrap();
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TARBALL_REVISION_MISMATCH");
}

#[tokio::test]
async fn accepts_an_advertised_historical_revision() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let packument = serde_json::json!({
        "name": "revision-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "revision-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": revision_integrity(REVISION_TWO_DIGEST).to_string(),
                    "tarball": format!("{registry}-/tarballs/sha512/{REVISION_TWO_DIGEST}"),
                    "revision": 2,
                    "revisions": [{
                        "revision": 1,
                        "integrity": revision_integrity(REVISION_ONE_DIGEST).to_string(),
                        "tarball": format!("{registry}-/tarballs/sha512/{REVISION_ONE_DIGEST}"),
                        "manifest": {},
                    }, {
                        "revision": 2,
                        "integrity": revision_integrity(REVISION_TWO_DIGEST).to_string(),
                        "tarball": format!("{registry}-/tarballs/sha512/{REVISION_TWO_DIGEST}"),
                        "manifest": {},
                    }],
                }
            }
        },
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" }
    });
    server
        .mock("GET", "/revision-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(1);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Registry(RegistryResolution {
        integrity: revision_integrity(REVISION_ONE_DIGEST),
        revision: Some(TarballRevision::try_from(1).unwrap()),
    });
    let name = "revision-pkg".parse::<PkgName>().unwrap();
    assert_eq!(verifier.verify(&resolution, ctx(&name, "1.0.0")).await, ResolutionVerification::Ok);
}

#[tokio::test]
async fn verify_short_circuits_non_registry_resolution() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24 * 365);
    let verifier = create_npm_resolution_verifier(opts);
    let directory = LockfileResolution::Directory(pnpm_lockfile::DirectoryResolution {
        directory: "/some/path".into(),
    });
    let name: PkgName = "acme".parse().expect("parse");
    let result = verifier.verify(&directory, ctx(&name, "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// Git-host archive URLs pin a full commit SHA, and pnpm never
/// recorded an integrity for them, so they stay exempt — recognized
/// from the URL even on a lockfile that omits the `gitHosted` marker.
#[tokio::test]
async fn git_hosted_archive_url_stays_exempt_without_the_flag() {
    let verifier =
        create_npm_resolution_verifier(default_opts("http://nonexistent.example.invalid/"));
    let tarball = "https://codeload.github.com/kevva/is-negative/tar.gz/0123456789abcdef0123456789abcdef01234567";
    let resolution = tarball_resolution(tarball, None);
    let name: PkgName = "is-negative".parse().expect("parse");
    assert!(!verifier.might_verify(&resolution, ctx(&name, tarball)));
    let result = verifier.verify(&resolution, ctx(&name, tarball)).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn unplanned_entry_sends_no_head_probe() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _head_mock = server.mock("HEAD", "/acme").expect(0).create_async().await;
    // The metadata-backed chain answers instead: the abbreviated
    // `modified` shortcut passes on an old package whose pinned
    // version the versions map still lists.
    let _meta_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(min_age_packument_json("acme", "1.0.0", "2024-01-01T00:00:00.000Z").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let planned = pnpm_resolving_resolver_base::PlannedCanonicalFetches::default();
    planned
        .set(std::collections::HashSet::from([("other".to_string(), "2.0.0".to_string(), None)]))
        .expect("first fill");
    opts.planned_canonical_fetches = Some(std::sync::Arc::clone(&planned));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn verify_routes_via_named_registry_prefix() {
    let mut server = mockito::Server::new_async().await;
    let server_url = server.url();
    let _attestation_mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    // The packument lists the same tarball URL the lockfile pins, so the
    // tarball-URL binding passes and the test stays focused on registry
    // routing.
    let packument = serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2024-01-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("{server_url}/acme/-/acme-1.0.0.tgz"),
                }
            }
        }
    });
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let mut named = HashMap::new();
    named.insert("internal".to_string(), format!("{server_url}/"));
    // Default registry is bogus — if the named-registry routing
    // breaks, the request would target the bogus URL and the test
    // would fail with a connection error instead of finding the mock.
    let mut opts = default_opts("http://nonexistent.example.invalid/");
    opts.registries_by_prefix = named;
    opts.minimum_release_age = Some(60 * 24);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let tarball = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{server_url}/acme/-/acme-1.0.0.tgz"),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let result =
        verifier.verify(&tarball, ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// `policy()` returns the snapshot the verification cache hashes
/// alongside the lockfile. Each field is sorted/deduped where the
/// snapshot contract requires it.
#[test]
fn policy_snapshot_records_all_fields_sorted_and_deduped() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24);
    opts.minimum_release_age_exclude_patterns =
        vec!["lodash".to_string(), "acme".to_string(), "lodash".to_string()];
    opts.minimum_release_age_exclude = Some(
        create_package_version_policy(["lodash".to_string(), "acme".to_string()]).expect("policy"),
    );
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    opts.trust_policy_exclude_patterns = vec!["@scope/foo".to_string()];
    opts.trust_policy_exclude =
        Some(create_package_version_policy(["@scope/foo".to_string()]).expect("policy"));
    opts.trust_policy_ignore_after = Some(60 * 24 * 30);
    let verifier = create_npm_resolution_verifier(opts);

    let policy = verifier.policy();
    // The two unconditional structural rules mark themselves in the
    // snapshot so a pre-rule cache record fails `can_trust_past_check`.
    assert_eq!(policy.get("tarballUrlBinding").and_then(serde_json::Value::as_bool), Some(true));
    assert_eq!(policy.get("integrityRequired").and_then(serde_json::Value::as_bool), Some(true));
    assert_eq!(policy.get("minimumReleaseAge").and_then(serde_json::Value::as_u64), Some(60 * 24));
    let min_age_excludes =
        policy.get("minimumReleaseAgeExclude").and_then(|value| value.as_array()).expect("array");
    assert_eq!(
        min_age_excludes
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect::<Vec<_>>(),
        vec!["acme".to_string(), "lodash".to_string()],
        "sorted + deduped",
    );
    assert_eq!(policy.get("trustPolicy").and_then(|value| value.as_str()), Some("no-downgrade"));
    assert_eq!(
        policy.get("trustPolicyIgnoreAfter").and_then(serde_json::Value::as_u64),
        Some(60 * 24 * 30),
    );
    assert_eq!(
        policy.get("minimumReleaseAgeIgnoreMissingTime").and_then(serde_json::Value::as_bool),
        Some(false),
    );
}

/// Concurrent verifications of the same `(registry, name, version)`
/// share one in-flight fetch — the lookup-context caches store
/// `Arc<OnceCell<…>>`, so 16 racing callers issue at most one
/// abbreviated GET. Without the singleflight property the verifier
/// regressed to N fetches per fan-out batch, which mockito's
/// `.expect(1)` catches.
#[tokio::test]
async fn concurrent_verifications_share_one_fetch() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    // The abbreviated-modified shortcut answers the gate without
    // touching the attestation or full-meta layers, so a single
    // `.expect(1)` exhaustively pins the per-fan-out fetch count for
    // the lookup chain.
    let abbreviated_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(
            abbreviated_packument_json("acme", "1.0.0", "2024-01-01T00:00:00.000Z").to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24); // 1 day
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "acme".parse().expect("parse");
    let resolution = registry_resolution();
    let results = futures_util::future::join_all(
        (0..16).map(|_| verifier.verify(&resolution, ctx(&name, "1.0.0"))),
    )
    .await;
    for result in results {
        assert_eq!(result, ResolutionVerification::Ok);
    }
    abbreviated_mock.assert_async().await;
}

/// Same registry document, flag unset: the verifier still passes, but
/// only by escalating to the full-packument fetch — a second request
/// for the same document. Guards both directions: the new step never
/// runs without the flag, and the request the flag saves is real.
#[tokio::test]
async fn without_registry_supports_time_field_abbreviated_time_is_not_consulted() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let now = Utc::now();
    let meta = serde_json::json!({
        "name": "aged-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "modified": now.to_rfc3339(),
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "aged-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("{server_url}/aged-pkg/-/aged-pkg-1.0.0.tgz"),
                }
            }
        }
    });
    let meta_mock = server
        .mock("GET", "/aged-pkg")
        .with_status(200)
        .with_body(meta.to_string())
        // Abbreviated fetch for the modified shortcut, then the
        // full-packument fallback for the per-version timestamp.
        .expect(2)
        .create_async()
        .await;
    // Without the flag the per-version fallbacks run in order, so the
    // attestation endpoint is consulted (and 404s) before the full
    // packument. Asserting it makes the escalation the flag avoids explicit.
    let attestation_mock = server
        .mock("GET", "/-/npm/v1/attestations/aged-pkg@1.0.0")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{server_url}/aged-pkg/-/aged-pkg-1.0.0.tgz"),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "aged-pkg".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);
    meta_mock.assert_async().await;
    attestation_mock.assert_async().await;
}
