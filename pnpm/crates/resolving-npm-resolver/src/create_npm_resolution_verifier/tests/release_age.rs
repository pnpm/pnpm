use super::{
    FAKE_INTEGRITY, PkgName, ResolutionVerification, abbreviated_packument_json, assert_eq,
    create_npm_resolution_verifier, create_package_version_policy, ctx, default_opts,
    min_age_packument_json, now_at, registry_resolution,
};
use pnpm_resolving_resolver_base::ResolutionVerifier;

/// `minimum_release_age = 0` keeps the age check inactive. The bogus
/// registry URL is a tripwire: a fetch would fail, so the `Ok` result
/// proves the verifier never attempted an age lookup.
#[tokio::test]
async fn min_age_zero_keeps_age_check_inactive() {
    let mut opts = default_opts("http://nonexistent.example.invalid/");
    opts.minimum_release_age = Some(0);
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn verify_skips_age_check_when_package_excluded() {
    // No mockito needed: if the exclude were ignored, the verifier
    // would issue a network call to the bogus URL and fail.
    let mut opts = default_opts("http://nonexistent.example.invalid/");
    opts.minimum_release_age = Some(60 * 24 * 365);
    opts.minimum_release_age_exclude =
        Some(create_package_version_policy(["acme".to_string()]).expect("policy"));
    opts.minimum_release_age_exclude_patterns = vec!["acme".to_string()];
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = registry_resolution();
    let name: PkgName = "acme".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn verify_skips_age_check_when_package_matches_exclude_pattern() {
    let mut opts = default_opts("http://nonexistent.example.invalid/");
    opts.minimum_release_age = Some(60 * 24 * 365);
    opts.minimum_release_age_exclude =
        Some(create_package_version_policy(["acme-*".to_string()]).expect("policy"));
    opts.minimum_release_age_exclude_patterns = vec!["acme-*".to_string()];
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "acme-widget".parse().expect("parse");

    let result = verifier.verify(&registry_resolution(), ctx(&name, "1.0.0")).await;

    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn verify_skips_age_check_for_an_exact_version_in_a_union() {
    let mut opts = default_opts("http://nonexistent.example.invalid/");
    opts.minimum_release_age = Some(60 * 24 * 365);
    opts.minimum_release_age_exclude =
        Some(create_package_version_policy(["acme@1.0.0 || 1.1.0".to_string()]).expect("policy"));
    opts.minimum_release_age_exclude_patterns = vec!["acme@1.0.0 || 1.1.0".to_string()];
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "acme".parse().expect("parse");

    let result = verifier.verify(&registry_resolution(), ctx(&name, "1.1.0")).await;

    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn min_age_pass_when_published_before_cutoff() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    // Attestation endpoint returns 404, forcing the full-metadata
    // layer to answer.
    let _attestation_mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let _full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_body(min_age_packument_json("acme", "1.0.0", "2024-01-01T00:00:00.000Z").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24); // 1 day
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn min_age_fail_when_published_within_cutoff() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _attestation_mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(min_age_packument_json("acme", "1.0.0", "2025-11-30T22:00:00.000Z").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24); // 1 day
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    let ResolutionVerification::Err { code, reason } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "MINIMUM_RELEASE_AGE_VIOLATION");
    assert!(reason.contains("within the minimumReleaseAge cutoff"), "got reason: {reason}");
}

#[tokio::test]
async fn min_age_missing_time_fails_closed_by_default() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let body = serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0.tgz"
                }
            }
        }
    });
    let _attestation_mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(body.to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    let ResolutionVerification::Err { code, reason } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "MINIMUM_RELEASE_AGE_VIOLATION");
    assert!(
        reason.contains("could not be checked against minimumReleaseAge"),
        "got reason: {reason}",
    );
}

#[tokio::test]
async fn min_age_missing_time_passes_when_ignored() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let body = serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0.tgz"
                }
            }
        }
    });
    let _attestation_mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(body.to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24);
    opts.ignore_missing_time_field = true;
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// The opt-in speaks for a registry that cannot date its releases, not
/// for a pin it has never heard of: a packument that dates every version
/// it lists is saying this one is not among them.
#[tokio::test]
async fn min_age_unlisted_version_fails_when_missing_time_is_ignored() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _attestation_mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.1")
        .with_status(404)
        .create_async()
        .await;
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(min_age_packument_json("acme", "1.0.0", "2025-01-01T00:00:00.000Z").to_string())
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24);
    opts.ignore_missing_time_field = true;
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.1"))
        .await;
    let ResolutionVerification::Err { code, reason } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "MINIMUM_RELEASE_AGE_VIOLATION");
    assert!(
        reason.contains("could not be checked against minimumReleaseAge"),
        "got reason: {reason}",
    );
}

/// A previously-cached run with a stricter (larger) cutoff stays
/// trustworthy under today's looser policy — the set of accepted
/// versions is a subset of today's.
#[test]
fn can_trust_past_check_accepts_looser_min_age() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24); // today: 1 day
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = verifier.policy().clone();
    cached.insert("minimumReleaseAge".to_string(), (60 * 24 * 7).into()); // past: 7 days
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(verifier.can_trust_past_check(&cached));
}

/// Tightening the cutoff invalidates the cached run — versions
/// that passed under a looser cutoff may now be in the new
/// (narrower) window.
#[test]
fn can_trust_past_check_rejects_tighter_min_age() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24 * 7); // today: 7 days
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = serde_json::Map::new();
    cached.insert("tarballUrlBinding".to_string(), true.into());
    cached.insert("integrityRequired".to_string(), true.into());
    cached.insert("minimumReleaseAge".to_string(), (60 * 24).into()); // past: 1 day
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}

/// Abbreviated-modified shortcut: when the package-level `modified`
/// timestamp is older than the cutoff and the pinned version is
/// still listed, the shortcut passes the gate without falling
/// through to the attestation or full-meta layers.
#[tokio::test]
async fn min_age_pass_via_abbreviated_modified_shortcut() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _abbreviated_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
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
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// The shortcut is upper-bounded by `modified`: a package whose
/// `modified` is within the cutoff window may still have older
/// versions, so the shortcut must yield and let the full chain
/// answer. This test pins the fall-through by mocking BOTH the
/// abbreviated GET (returning a recent `modified`) and the full
/// GET (returning an older per-version `time`); the verifier must
/// pass via the full path even though the abbreviated one couldn't
/// decide.
#[tokio::test]
async fn min_age_shortcut_falls_through_when_modified_within_cutoff() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _abbreviated_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        .with_body(
            // `modified` is well within the 1-day cutoff (the policy's `now`),
            // so the shortcut cannot decide.
            abbreviated_packument_json("acme", "1.0.0", "2025-11-30T23:30:00.000Z").to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let _attestation_mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let _full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_body(min_age_packument_json("acme", "1.0.0", "2024-01-01T00:00:00.000Z").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24); // 1 day
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// The shortcut treats `modified` as an upper bound only for
/// versions the registry currently lists. An unpublished or
/// never-published pin must NOT slip through on a stale
/// package-level timestamp — the verifier falls through to the
/// per-version layers, which surface the unchecked entry.
#[tokio::test]
async fn min_age_shortcut_falls_through_when_version_not_listed() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _abbreviated_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        // `modified` is old enough, but the abbreviated packument
        // only lists `1.0.0` — the verifier is checking `2.0.0`.
        .with_body(
            abbreviated_packument_json("acme", "1.0.0", "2024-01-01T00:00:00.000Z").to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let _attestation_mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@2.0.0")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let _full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        // Full meta also lacks 2.0.0; the verifier falls through to
        // the missing-time-field branch (`ignore_missing_time_field`
        // is false by default, so this yields
        // `MINIMUM_RELEASE_AGE_VIOLATION`).
        .with_body(min_age_packument_json("acme", "1.0.0", "2024-01-01T00:00:00.000Z").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "2.0.0"))
        .await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "MINIMUM_RELEASE_AGE_VIOLATION");
}
