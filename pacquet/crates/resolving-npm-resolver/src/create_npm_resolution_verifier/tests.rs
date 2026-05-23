use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use pacquet_config::{TrustPolicy, version_policy::create_package_version_policy};
use pacquet_lockfile::{LockfileResolution, PkgName, RegistryResolution, TarballResolution};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_resolving_resolver_base::{ResolutionVerification, ResolutionVerifier, VerifyCtx};
use pretty_assertions::assert_eq;
use ssri::Integrity;

use super::{CreateNpmResolutionVerifierOptions, create_npm_resolution_verifier};

const FAKE_INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

fn now_at(date: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(date).expect("parse rfc3339").with_timezone(&Utc)
}

fn fake_integrity() -> Integrity {
    FAKE_INTEGRITY.parse::<Integrity>().expect("parse fake integrity")
}

fn registry_resolution() -> LockfileResolution {
    LockfileResolution::Registry(RegistryResolution { integrity: fake_integrity() })
}

fn registries_with_default(default: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("default".to_string(), default.to_string());
    map
}

/// Build a default `CreateNpmResolutionVerifierOptions` with the
/// given registry URL. Tests override individual fields after.
fn default_opts(registry_url: &str) -> CreateNpmResolutionVerifierOptions {
    CreateNpmResolutionVerifierOptions {
        minimum_release_age: None,
        minimum_release_age_exclude: None,
        minimum_release_age_exclude_patterns: Vec::new(),
        ignore_missing_time_field: false,
        trust_policy: None,
        trust_policy_exclude: None,
        trust_policy_exclude_patterns: Vec::new(),
        trust_policy_ignore_after: None,
        registries: registries_with_default(registry_url),
        named_registries: HashMap::new(),
        http_client: Arc::new(ThrottledClient::default()),
        auth_headers: Arc::new(AuthHeaders::default()),
        cache_dir: None,
        meta_cache: None,
        now: None,
    }
}

/// Wire-shape full-metadata document with a single `time` slot and
/// no provenance. Used for the minimumReleaseAge path; the trust
/// check needs a richer fixture (see `trust_packument_json`).
fn min_age_packument_json(name: &str, version: &str, published_at: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "dist-tags": { "latest": version },
        "time": { version: published_at },
        "versions": {
            version: {
                "name": name,
                "version": version,
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("https://registry/{name}-{version}.tgz"),
                }
            }
        }
    })
}

/// Packument with two versions: earlier (`prior_version`) has
/// `_npmUser.trustedPublisher`, current has only `dist.attestations.provenance`.
/// This is the canonical "trusted-publisher → provenance" downgrade.
fn trust_downgrade_packument(name: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "dist-tags": { "latest": "1.1.0" },
        "time": {
            "1.0.0": "2025-01-01T00:00:00.000Z",
            "1.1.0": "2025-02-01T00:00:00.000Z"
        },
        "versions": {
            "1.0.0": {
                "name": name,
                "version": "1.0.0",
                "_npmUser": { "name": "alice", "trustedPublisher": { "id": "github", "oidcConfigId": "release" } },
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("https://registry/{name}-1.0.0.tgz")
                }
            },
            "1.1.0": {
                "name": name,
                "version": "1.1.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("https://registry/{name}-1.1.0.tgz"),
                    "attestations": { "provenance": { "predicateType": "https://slsa.dev/provenance/v1" } }
                }
            }
        }
    })
}

/// Packument where every published version carries the same
/// (provenance) evidence — verifying any of them must NOT raise
/// a trust downgrade.
fn stable_trust_packument(name: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "dist-tags": { "latest": "1.1.0" },
        "time": {
            "1.0.0": "2025-01-01T00:00:00.000Z",
            "1.1.0": "2025-02-01T00:00:00.000Z"
        },
        "versions": {
            "1.0.0": {
                "name": name,
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("https://registry/{name}-1.0.0.tgz"),
                    "attestations": { "provenance": { "predicateType": "https://slsa.dev/provenance/v1" } }
                }
            },
            "1.1.0": {
                "name": name,
                "version": "1.1.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("https://registry/{name}-1.1.0.tgz"),
                    "attestations": { "provenance": { "predicateType": "https://slsa.dev/provenance/v1" } }
                }
            }
        }
    })
}

/// No-op `ctx` builder that ties the borrowed `name` to the call
/// site's lifetime.
fn ctx<'a>(name: &'a PkgName, version: &'a str) -> VerifyCtx<'a> {
    VerifyCtx { name, version }
}

/// `create_npm_resolution_verifier` returns `None` when neither
/// the maturity nor trust check is active — the install side then
/// skips fan-out entirely.
#[test]
fn returns_none_when_no_policy_active() {
    let server_url = "https://registry.example/";
    let opts = default_opts(server_url);
    assert!(create_npm_resolution_verifier(opts).is_none());
}

/// `minimum_release_age = 0` is treated the same as `None` — upstream's
/// `Boolean(opts.minimumReleaseAge)` returns `false` for `0`, so the
/// verifier stays inactive.
#[test]
fn returns_none_when_min_age_is_zero() {
    let server_url = "https://registry.example/";
    let mut opts = default_opts(server_url);
    opts.minimum_release_age = Some(0);
    assert!(create_npm_resolution_verifier(opts).is_none());
}

/// `trust_policy = Off` keeps the verifier inactive even when set
/// explicitly. Mirrors upstream's `opts.trustPolicy === 'no-downgrade'`
/// gating.
#[test]
fn returns_none_when_trust_policy_off() {
    let server_url = "https://registry.example/";
    let mut opts = default_opts(server_url);
    opts.trust_policy = Some(TrustPolicy::Off);
    assert!(create_npm_resolution_verifier(opts).is_none());
}

/// Setting `minimum_release_age` returns an active verifier even
/// without a trust policy.
#[test]
fn returns_some_when_min_age_set() {
    let server_url = "https://registry.example/";
    let mut opts = default_opts(server_url);
    opts.minimum_release_age = Some(60 * 24);
    assert!(create_npm_resolution_verifier(opts).is_some());
}

/// `trust_policy = NoDowngrade` alone activates the verifier.
#[test]
fn returns_some_when_trust_policy_no_downgrade() {
    let server_url = "https://registry.example/";
    let mut opts = default_opts(server_url);
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    assert!(create_npm_resolution_verifier(opts).is_some());
}

/// A git / directory / binary resolution short-circuits to
/// `Ok` without issuing any network calls — neither policy applies
/// outside the npm-registry protocol.
#[tokio::test]
async fn verify_short_circuits_non_registry_resolution() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24 * 365);
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");
    let directory = LockfileResolution::Directory(pacquet_lockfile::DirectoryResolution {
        directory: "/some/path".into(),
    });
    let name: PkgName = "acme".parse().expect("parse");
    let result = verifier.verify(&directory, ctx(&name, "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// Non-semver version (URL spec, git ref, file: ref) → pass without
/// asking the registry. Mirrors upstream's `!semver.valid(version)`
/// gate.
#[tokio::test]
async fn verify_short_circuits_non_semver_version() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24 * 365);
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");
    let resolution = registry_resolution();
    let name: PkgName = "acme".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "not-semver")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// When the exclude policy covers the package, age check skips —
/// the version is treated as opted out regardless of its publish
/// timestamp.
#[tokio::test]
async fn verify_skips_age_check_when_package_excluded() {
    // No mockito needed: if the exclude were ignored, the verifier
    // would issue a network call to the bogus URL and fail.
    let mut opts = default_opts("http://nonexistent.example.invalid/");
    opts.minimum_release_age = Some(60 * 24 * 365);
    opts.minimum_release_age_exclude =
        Some(create_package_version_policy(["acme".to_string()]).expect("policy"));
    opts.minimum_release_age_exclude_patterns = vec!["acme".to_string()];
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");
    let resolution = registry_resolution();
    let name: PkgName = "acme".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// Full-metadata path: registry reports a publish time well before
/// the cutoff → verify returns `Ok`.
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
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// Within-cutoff publish time → fail with the verifier's
/// `MINIMUM_RELEASE_AGE_VIOLATION` code.
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
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    let ResolutionVerification::Err { code, reason } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "MINIMUM_RELEASE_AGE_VIOLATION");
    assert!(reason.contains("within the minimumReleaseAge cutoff"), "got reason: {reason}");
}

/// Registry strips per-version `time`. With `ignore_missing_time_field`
/// off (the default), the verifier fails closed.
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
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");
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

/// Opting in to `ignore_missing_time_field` flips the missing-time
/// case from a fail-closed violation to a pass. Mirrors upstream's
/// `minimumReleaseAgeIgnoreMissingTime` resolver flag.
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
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// `trust_policy = NoDowngrade` rejects a version whose evidence is
/// weaker than an earlier-published version's. Earlier 1.0.0 had
/// `trustedPublisher`; current 1.1.0 has only `provenance` →
/// `TRUST_DOWNGRADE`.
#[tokio::test]
async fn trust_downgrade_publisher_to_provenance_fails() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(trust_downgrade_packument("acme").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.1.0"))
        .await;
    let ResolutionVerification::Err { code, reason } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TRUST_DOWNGRADE");
    assert!(reason.contains("trust downgrade"), "got reason: {reason}");
}

/// When every prior version's evidence equals or precedes the
/// current version's, the trust check passes.
#[tokio::test]
async fn trust_downgrade_pass_when_no_weaker_evidence() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(stable_trust_packument("acme").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.1.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// Tarball resolutions whose URL falls under a named-registry
/// prefix route to that registry's metadata endpoint. Here `gh:` →
/// the mocked GitHub Packages base URL.
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
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(min_age_packument_json("acme", "1.0.0", "2024-01-01T00:00:00.000Z").to_string())
        .expect(1)
        .create_async()
        .await;

    let mut named = HashMap::new();
    named.insert("internal".to_string(), format!("{server_url}/"));
    // Default registry is bogus — if the named-registry routing
    // breaks, the request would target the bogus URL and the test
    // would fail with a connection error instead of finding the mock.
    let mut opts = default_opts("http://nonexistent.example.invalid/");
    opts.named_registries = named;
    opts.minimum_release_age = Some(60 * 24);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");
    let tarball = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{server_url}/acme/-/acme-1.0.0.tgz"),
        integrity: Some(fake_integrity()),
        git_hosted: None,
        path: None,
    });
    let result =
        verifier.verify(&tarball, ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// `policy()` returns the snapshot the verification cache hashes
/// alongside the lockfile. Each field is sorted/deduped where the
/// upstream contract requires it.
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
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");

    let policy = verifier.policy();
    assert_eq!(policy.get("minimumReleaseAge").and_then(|value| value.as_u64()), Some(60 * 24));
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
        policy.get("trustPolicyIgnoreAfter").and_then(|value| value.as_u64()),
        Some(60 * 24 * 30),
    );
}

/// A previously-cached run with a stricter (larger) cutoff stays
/// trustworthy under today's looser policy — the set of accepted
/// versions is a subset of today's.
#[test]
fn can_trust_past_check_accepts_looser_min_age() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24); // today: 1 day
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");

    let mut cached = serde_json::Map::new();
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
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");

    let mut cached = serde_json::Map::new();
    cached.insert("minimumReleaseAge".to_string(), (60 * 24).into()); // past: 1 day
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}

/// Any drift in the exclude list invalidates the cached run, even
/// when the drift would have been more permissive (an extra entry).
/// Mirrors upstream's stricter-than-necessary identity check.
#[test]
fn can_trust_past_check_rejects_changed_exclude_list() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24);
    opts.minimum_release_age_exclude_patterns = vec!["acme".to_string()];
    opts.minimum_release_age_exclude =
        Some(create_package_version_policy(["acme".to_string()]).expect("policy"));
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");

    let mut cached = serde_json::Map::new();
    cached.insert("minimumReleaseAge".to_string(), (60 * 24).into());
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}

/// Switching trust policy on or off invalidates the cached run.
#[test]
fn can_trust_past_check_rejects_changed_trust_policy() {
    let mut opts = default_opts("https://registry.example/");
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");

    let mut cached = serde_json::Map::new();
    cached.insert("minimumReleaseAge".to_string(), 0.into());
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}

/// Changing `trustPolicyIgnoreAfter` (or going from set to unset)
/// invalidates the cache.
#[test]
fn can_trust_past_check_rejects_changed_ignore_after() {
    let mut opts = default_opts("https://registry.example/");
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    opts.trust_policy_ignore_after = Some(60 * 24 * 14);
    let verifier = create_npm_resolution_verifier(opts).expect("verifier");

    let mut cached = serde_json::Map::new();
    cached.insert("minimumReleaseAge".to_string(), 0.into());
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::String("no-downgrade".into()));
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}
