use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use pacquet_config::{TrustPolicy, version_policy::create_package_version_policy};
use pacquet_lockfile::{LockfileResolution, PkgName, RegistryResolution, TarballResolution};
use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pacquet_resolving_resolver_base::{ResolutionVerification, ResolutionVerifier, VerifyCtx};
use pretty_assertions::assert_eq;
use ssri::Integrity;

use super::{
    CreateNpmResolutionVerifierOptions, create_npm_resolution_verifier, observed_dist_stats_sink,
};

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
        // No retries: tests that point an endpoint at an unmocked /
        // erroring upstream would otherwise wait out the full pnpm
        // backoff (10 s + 60 s) on every run.
        retry_opts: RetryOpts { retries: 0, ..RetryOpts::default() },
        now: None,
        observed_dist_stats: None,
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

/// Packument with two versions: earlier (`prior_version`) has both
/// `_npmUser.trustedPublisher` *and* `dist.attestations.provenance`
/// — `get_trust_evidence` only ranks the publisher flag as the
/// strongest evidence when the version also ships an attestation —
/// while current has only `dist.attestations.provenance`. This is the
/// canonical "trusted-publisher → provenance" downgrade.
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

/// The tarball-URL binding is unconditional: even with no
/// minimumReleaseAge / trustPolicy configured, an entry whose pinned
/// tarball URL doesn't match the registry metadata is rejected.
#[tokio::test]
async fn verifies_tarball_url_when_no_policy_active() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let packument = serde_json::json!({
        "name": "aged-pkg",
        "dist-tags": { "latest": "1.0.0" },
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
    let _meta_mock = server
        .mock("GET", "/aged-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;
    // No minimumReleaseAge, no trustPolicy.
    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://attacker.example/aged-pkg-1.0.0.tgz".to_string(),
        integrity: Some(fake_integrity()),
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "aged-pkg".parse().expect("parse");
    assert!(verifier.might_verify(&resolution, ctx(&name, "1.0.0")));
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TARBALL_URL_MISMATCH");
}

#[tokio::test]
async fn registry_resolution_with_no_active_policy_skips_metadata_lookup() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _meta_mock = server.mock("GET", "/acme").expect(0).create_async().await;

    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "acme".parse().expect("parse");
    assert!(!verifier.might_verify(&registry_resolution(), ctx(&name, "1.0.0")));
    let result = verifier.verify(&registry_resolution(), ctx(&name, "1.0.0")).await;

    assert_eq!(result, ResolutionVerification::Ok);
}

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

/// `trust_policy = Off` keeps the trust check inactive (same tripwire
/// rationale as the age-check test above).
#[tokio::test]
async fn trust_off_keeps_trust_check_inactive() {
    let mut opts = default_opts("http://nonexistent.example.invalid/");
    opts.trust_policy = Some(TrustPolicy::Off);
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// A git / directory / binary resolution short-circuits to
/// `Ok` without issuing any network calls — neither policy applies
/// outside the npm-registry protocol.
#[tokio::test]
async fn verify_short_circuits_non_registry_resolution() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24 * 365);
    let verifier = create_npm_resolution_verifier(opts);
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
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = registry_resolution();
    let name: PkgName = "acme".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "not-semver")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// `file:` tarball resolutions are local artifacts, not registry
/// entries, so the verifier must skip minimumReleaseAge/trust checks.
#[tokio::test]
async fn verify_short_circuits_file_tarball_resolution() {
    let mut opts = default_opts("http://nonexistent.example.invalid/");
    opts.minimum_release_age = Some(60 * 24 * 365);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "file:vendor/types__my-cool-lib-v1.0.0.tgz".to_string(),
        integrity: Some(fake_integrity()),
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "@types/my-cool-lib".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// A registry entry whose pinned tarball URL is not the artifact the
/// registry's metadata lists is rejected before the age check passes it.
/// Guards against a tampered lockfile pairing an aged, trusted
/// name@version with attacker-hosted bytes.
#[tokio::test]
async fn verify_flags_tarball_url_mismatch() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let packument = serde_json::json!({
        "name": "aged-pkg",
        "dist-tags": { "latest": "1.0.0" },
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
    let _meta_mock = server
        .mock("GET", "/aged-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://attacker.example/aged-pkg-1.0.0.tgz".to_string(),
        integrity: Some(fake_integrity()),
        git_hosted: None,
        path: None,
    });
    let result = verifier
        .verify(&resolution, ctx(&"aged-pkg".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    let ResolutionVerification::Err { code, reason } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TARBALL_URL_MISMATCH");
    assert!(
        reason.contains("does not match the registry's published metadata"),
        "got reason: {reason}",
    );
}

/// A lockfile URL that differs from the registry metadata only by an
/// explicit default port and the http/https scheme is a benign
/// normalization, not tampering — `same_tarball_url` must canonicalize
/// it away (this is what `canonical_tarball_url`'s URL parse buys over a
/// plain string compare).
#[tokio::test]
async fn tarball_url_default_port_and_scheme_difference_is_a_match() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    // The served metadata lists the artifact on a different host with an
    // explicit default port and the http scheme; the lockfile pins the
    // canonical https/no-port form of the same URL.
    let packument = serde_json::json!({
        "name": "aged-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "aged-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "http://registry.npmjs.org:80/aged-pkg/-/aged-pkg-1.0.0.tgz",
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/aged-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;
    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://registry.npmjs.org/aged-pkg/-/aged-pkg-1.0.0.tgz".to_string(),
        integrity: Some(fake_integrity()),
        git_hosted: None,
        path: None,
    });
    let result = verifier
        .verify(&resolution, ctx(&"aged-pkg".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
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
    let verifier = create_npm_resolution_verifier(opts);
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
    let verifier = create_npm_resolution_verifier(opts);
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
    let verifier = create_npm_resolution_verifier(opts);
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
    let verifier = create_npm_resolution_verifier(opts);
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
    let verifier = create_npm_resolution_verifier(opts);
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
    opts.named_registries = named;
    opts.minimum_release_age = Some(60 * 24);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
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
    let verifier = create_npm_resolution_verifier(opts);

    let policy = verifier.policy();
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
}

/// A previously-cached run with a stricter (larger) cutoff stays
/// trustworthy under today's looser policy — the set of accepted
/// versions is a subset of today's.
#[test]
fn can_trust_past_check_accepts_looser_min_age() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24); // today: 1 day
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = serde_json::Map::new();
    cached.insert("tarballUrlBinding".to_string(), true.into());
    cached.insert("minimumReleaseAge".to_string(), (60 * 24 * 7).into()); // past: 7 days
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(verifier.can_trust_past_check(&cached));
}

/// A cache record that predates the tarball-URL binding rule (no
/// `tarballUrlBinding` marker) can't be trusted to have enforced it,
/// so it's rejected and forces a re-verification.
#[test]
fn can_trust_past_check_rejects_missing_tarball_url_binding() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24);
    let verifier = create_npm_resolution_verifier(opts);

    // Otherwise-compatible cached policy, but without the binding marker.
    let mut cached = serde_json::Map::new();
    cached.insert("minimumReleaseAge".to_string(), (60 * 24 * 7).into());
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
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
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = serde_json::Map::new();
    cached.insert("tarballUrlBinding".to_string(), true.into());
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
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = serde_json::Map::new();
    cached.insert("tarballUrlBinding".to_string(), true.into());
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
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = serde_json::Map::new();
    cached.insert("tarballUrlBinding".to_string(), true.into());
    cached.insert("minimumReleaseAge".to_string(), 0.into());
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::String("no-downgrade".into()));
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}

/// Wire-shape **abbreviated** packument with a package-level
/// `modified` timestamp and a `versions` map listing the candidate
/// version. The abbreviated form omits per-version `time`; the
/// shortcut layer reads only the `modified` and the `versions` key
/// set, so this is the minimal fixture the shortcut needs.
fn abbreviated_packument_json(name: &str, version: &str, modified: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "modified": modified,
        "dist-tags": { "latest": version },
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

/// Abbreviated-modified shortcut: when the package-level `modified`
/// timestamp is older than the cutoff and the pinned version is
/// still listed, the shortcut passes the gate without falling
/// through to the attestation or full-meta layers. Mirrors
/// upstream's
/// [`tryAbbreviatedModifiedShortcut`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L606-L624)
/// happy path.
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
/// per-version layers, which surface the unchecked entry. Mirrors
/// upstream's
/// [`if (!meta?.versionNames?.has(version)) return undefined`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L622)
/// guard.
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

/// The binding check records each verified entry's `dist.unpackedSize`
/// and `dist.fileCount` into the `observed_dist_stats` sink when one is
/// provided.
#[tokio::test]
async fn binding_check_records_dist_stats_into_the_sink() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let tarball_url = format!("{server_url}/acme/-/acme-1.0.0.tgz");
    let packument = serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "tarball": tarball_url,
                    "unpackedSize": 123_456,
                    "fileCount": 42,
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;

    let sink = observed_dist_stats_sink();
    let mut opts = default_opts(&registry);
    opts.observed_dist_stats = Some(Arc::clone(&sink));
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: tarball_url.clone(),
        integrity: Some(fake_integrity()),
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "acme".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;

    assert_eq!(result, ResolutionVerification::Ok);
    let recorded = sink
        .get(&("acme".to_string(), "1.0.0".to_string()))
        .map(|entry| *entry.value())
        .expect("stats recorded");
    assert_eq!(recorded.unpacked_size, Some(123_456));
    assert_eq!(recorded.file_count, Some(42));
}
