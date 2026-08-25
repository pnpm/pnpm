use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use pnpm_config::{TrustPolicy, version_policy::create_package_version_policy};
use pnpm_lockfile::{
    LockfileResolution, PkgName, RegistryResolution, TarballResolution, TarballRevision,
};
use pnpm_network::{
    AuthHeaders, MetadataCacheScope, RetryOpts, ThrottledClient, UpstreamRouteHook,
};
use pnpm_registry::Package;
use pnpm_resolving_resolver_base::{ResolutionVerification, ResolutionVerifier, VerifyCtx};
use pretty_assertions::assert_eq;
use ssri::Integrity;
use tempfile::TempDir;

use super::{
    CreateNpmResolutionVerifierOptions, create_npm_resolution_verifier, observed_dist_stats_sink,
};
use crate::{
    mirror::{ABBREVIATED_META_DIR, get_pkg_mirror_path, load_meta},
    persist_meta_to_mirror,
};

const FAKE_INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

fn now_at(date: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(date).expect("parse rfc3339").with_timezone(&Utc)
}

fn fake_integrity() -> Integrity {
    FAKE_INTEGRITY.parse::<Integrity>().expect("parse fake integrity")
}

fn registry_resolution() -> LockfileResolution {
    LockfileResolution::Registry(RegistryResolution { integrity: fake_integrity(), revision: None })
}

fn tarball_resolution(tarball: &str, integrity: Option<Integrity>) -> LockfileResolution {
    LockfileResolution::Tarball(TarballResolution {
        tarball: tarball.to_string(),
        integrity,
        revision: None,
        git_hosted: None,
        path: None,
    })
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
        registry_supports_time_field: false,
        trust_policy: None,
        trust_policy_exclude: None,
        trust_policy_exclude_patterns: Vec::new(),
        trust_policy_ignore_after: None,
        registries: registries_with_default(registry_url),
        registries_by_prefix: HashMap::new(),
        http_client: Arc::new(ThrottledClient::default()),
        auth_headers: Arc::new(AuthHeaders::default()),
        cache_dir: None,
        meta_cache: None,
        offline: false,
        // No retries: tests that point an endpoint at an unmocked /
        // erroring upstream would otherwise wait out the full pnpm
        // backoff (10 s + 60 s) on every run.
        retry_opts: RetryOpts { retries: 0, ..RetryOpts::default() },
        now: None,
        observed_dist_stats: None,
        planned_canonical_fetches: None,
    }
}

struct ScopeHook {
    scope: MetadataCacheScope,
}

impl UpstreamRouteHook for ScopeHook {
    fn authorize(&self, _url: &str, _package: Option<&str>) -> Option<String> {
        None
    }

    fn metadata_scope(&self, _url: &str, _package: Option<&str>) -> MetadataCacheScope {
        self.scope.clone()
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
    VerifyCtx { name, version, registry_name: None }
}

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
    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://attacker.example/aged-pkg-1.0.0.tgz".to_string(),
        integrity: Some(fake_integrity()),
        revision: None,
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

const REVISION_ONE_DIGEST: &str =
    "Umd2iCLuYk1I_OFexcp5y9YCy39MIVelFlVpkfIu-Me173sY0f9BxZNw77CFhlHUSpNsEbexRMSP4E3zxqPo2g";
const REVISION_TWO_DIGEST: &str =
    "rMKNsr63tCuqHLAkPUAcy04_zkTXsCh5pSeZqt_1QVItiCJZiy-mZPnVFWwAySSAXXXDhovVbCrLgdN-mONa3A";

fn revision_integrity(digest: &str) -> Integrity {
    format!("sha512-{}==", digest.replace('_', "/").replace('-', "+"))
        .parse()
        .expect("revision integrity")
}

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
async fn rejects_a_revision_with_an_unadvertised_integrity() {
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
        integrity: revision_integrity(REVISION_TWO_DIGEST),
        revision: Some(TarballRevision::try_from(1).unwrap()),
    });
    let name = "revision-pkg".parse::<PkgName>().unwrap();
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected revision mismatch, got {result:?}");
    };
    assert_eq!(code, "TARBALL_REVISION_MISMATCH");
}

#[tokio::test]
async fn private_scope_verifier_ignores_public_mirror_and_writes_private_mirror() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let public_tarball = format!("{server_url}/acme/-/acme-1.0.0.tgz");
    let private_tarball = format!("{server_url}/acme/-/acme-private-1.0.0.tgz");
    let public_packument = serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": public_tarball,
                }
            }
        }
    });
    let private_packument = serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": private_tarball,
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(private_packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let cache = TempDir::new().expect("tempdir");
    let public_meta: Package = serde_json::from_value(public_packument).expect("package parses");
    persist_meta_to_mirror(cache.path(), ABBREVIATED_META_DIR, &registry, &public_meta)
        .expect("warm public mirror");

    let mut opts = default_opts(&registry);
    opts.cache_dir = Some(cache.path().to_path_buf());
    opts.auth_headers =
        Arc::new(AuthHeaders::default().with_route_hook(Arc::new(ScopeHook {
            scope: MetadataCacheScope::Private { descriptor_id: "private-scope".to_string() },
        }) as Arc<dyn UpstreamRouteHook>));
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: public_tarball.clone(),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "acme".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;

    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected private metadata mismatch, got {result:?}");
    };
    assert_eq!(code, "TARBALL_URL_MISMATCH");

    let private_path = get_pkg_mirror_path(
        cache.path(),
        "v11/metadata-private/private-scope/metadata",
        &registry,
        "acme",
    )
    .expect("private mirror path");
    let private_meta = load_meta(&private_path).expect("private mirror written");
    let private_version = private_meta.versions.get("1.0.0").expect("private version");
    assert_eq!(private_version.dist.tarball, private_tarball);

    let public_path =
        get_pkg_mirror_path(cache.path(), ABBREVIATED_META_DIR, &registry, "acme").expect("path");
    let public_meta = load_meta(&public_path).expect("public mirror remains readable");
    let public_version = public_meta.versions.get("1.0.0").expect("public version");
    assert_eq!(public_version.dist.tarball, public_tarball);
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

#[tokio::test]
async fn verify_short_circuits_file_tarball_resolution() {
    let mut opts = default_opts("http://nonexistent.example.invalid/");
    opts.minimum_release_age = Some(60 * 24 * 365);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution =
        tarball_resolution("file:vendor/types__my-cool-lib-v1.0.0.tgz", Some(fake_integrity()));
    let name: PkgName = "@types/my-cool-lib".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// A remote tarball that pins no hash can't be checked against
/// anything once downloaded, so the verifier refuses it before the
/// fetch pass ever sees it. The bogus registry URL is a tripwire: the
/// check is network-free, so no lookup may be attempted.
#[tokio::test]
async fn missing_integrity_is_rejected_before_any_metadata_lookup() {
    let verifier =
        create_npm_resolution_verifier(default_opts("http://nonexistent.example.invalid/"));
    let resolution = tarball_resolution("https://registry.example/foo/-/foo-1.0.0.tgz", None);
    let name: PkgName = "foo".parse().expect("parse");
    assert!(verifier.might_verify(&resolution, ctx(&name, "1.0.0")));
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    assert_eq!(
        result,
        ResolutionVerification::Err {
            code: "MISSING_TARBALL_INTEGRITY",
            reason: r#"has no "integrity" field, so its downloaded tarball cannot be verified"#
                .to_string(),
        },
    );
}

/// `integrity: ''` parses into zero hashes, which pins nothing — the
/// same verdict as an absent field.
#[tokio::test]
async fn empty_integrity_counts_as_missing() {
    let verifier =
        create_npm_resolution_verifier(default_opts("http://nonexistent.example.invalid/"));
    let empty = "".parse::<Integrity>().expect("empty integrity parses");
    let name: PkgName = "foo".parse().expect("parse");
    for resolution in [
        tarball_resolution("https://registry.example/foo/-/foo-1.0.0.tgz", Some(empty.clone())),
        LockfileResolution::Registry(RegistryResolution { integrity: empty, revision: None }),
    ] {
        let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
        let ResolutionVerification::Err { code, .. } = result else {
            panic!("expected Err, got {result:?}");
        };
        assert_eq!(code, "MISSING_TARBALL_INTEGRITY");
    }
}

/// URL-keyed deps carry their spec in the version slot, which skips
/// the registry policies — but not the missing-integrity check, whose
/// verdict doesn't depend on the registry's metadata.
#[tokio::test]
async fn missing_integrity_is_rejected_on_a_non_semver_version() {
    let verifier =
        create_npm_resolution_verifier(default_opts("http://nonexistent.example.invalid/"));
    let tarball = "https://cdn.example/foo/-/foo-1.0.0.tgz";
    let resolution = tarball_resolution(tarball, None);
    let name: PkgName = "foo".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, tarball)).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "MISSING_TARBALL_INTEGRITY");
}

/// The same URL-keyed entry passes once it pins a hash, without a
/// registry round-trip (the tripwire registry would fail one).
#[tokio::test]
async fn url_keyed_tarball_with_integrity_passes_without_a_lookup() {
    let verifier =
        create_npm_resolution_verifier(default_opts("http://nonexistent.example.invalid/"));
    let tarball = "https://cdn.example/foo/-/foo-1.0.0.tgz";
    let resolution = tarball_resolution(tarball, Some(fake_integrity()));
    let name: PkgName = "foo".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, tarball)).await;
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

/// The exemption is the URL's, not the flag's: a `gitHosted: true`
/// marker on an arbitrary URL, or a git-host URL that isn't pinned to
/// a commit, buys nothing.
#[tokio::test]
async fn integrity_is_required_despite_a_git_hosted_claim() {
    let verifier =
        create_npm_resolution_verifier(default_opts("http://nonexistent.example.invalid/"));
    let name: PkgName = "evil".parse().expect("parse");
    let forged = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://attacker.example/evil-1.0.0.tgz".to_string(),
        integrity: None,
        revision: None,
        git_hosted: Some(true),
        path: None,
    });
    let unpinned =
        tarball_resolution("https://codeload.github.com/kevva/is-negative/tar.gz/main", None);
    for resolution in [forged, unpinned] {
        let version = "https+++attacker.example+evil";
        assert!(verifier.might_verify(&resolution, ctx(&name, version)));
        let result = verifier.verify(&resolution, ctx(&name, version)).await;
        let ResolutionVerification::Err { code, .. } = result else {
            panic!("expected Err, got {result:?}");
        };
        assert_eq!(code, "MISSING_TARBALL_INTEGRITY");
    }
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
        revision: None,
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
    // canonical https/no-port form of the same URL. The host is deliberately
    // not a built-in named registry: one of those would route the metadata
    // fetch to that registry instead of this mock.
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
                    "tarball": "http://cdn.example.test:80/aged-pkg/-/aged-pkg-1.0.0.tgz",
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
        tarball: "https://cdn.example.test/aged-pkg/-/aged-pkg-1.0.0.tgz".to_string(),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let result = verifier
        .verify(&resolution, ctx(&"aged-pkg".parse::<PkgName>().expect("parse"), "1.0.0"))
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
async fn planned_fetch_head_shortcut_skips_the_metadata_body() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _head_mock = server
        .mock("HEAD", "/acme")
        .with_status(200)
        .with_header("last-modified", "Mon, 01 Jan 2024 00:00:00 GMT")
        .expect(1)
        .create_async()
        .await;
    // The whole point: no metadata body is fetched for a planned entry
    // whose package-level Last-Modified is older than the cutoff.
    let _meta_mock = server.mock("GET", "/acme").expect(0).create_async().await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let planned = pnpm_resolving_resolver_base::PlannedCanonicalFetches::default();
    planned
        .set(std::collections::HashSet::from([("acme".to_string(), "1.0.0".to_string(), None)]))
        .expect("first fill");
    opts.planned_canonical_fetches = Some(std::sync::Arc::clone(&planned));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
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

/// Same fixture as [`trust_downgrade_packument`] minus the `time` map:
/// a downgrade the check cannot see because it has no publish order to
/// walk.
fn time_free_trust_packument(name: &str) -> serde_json::Value {
    let mut body = trust_downgrade_packument(name);
    body.as_object_mut().expect("packument is an object").remove("time");
    body
}

#[tokio::test]
async fn trust_time_free_packument_fails_closed_by_default() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(time_free_trust_packument("acme").to_string())
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
    assert!(reason.contains(r#"missing the "time" field"#), "got reason: {reason}");
}

/// The same registry deficiency the age check already tolerates under
/// this opt-in: with no `time` map there is no publish order for the
/// downgrade walk to read, so the verifier passes the entry rather than
/// locking the user out of a registry that never serves the field.
#[tokio::test]
async fn trust_time_free_packument_passes_when_ignored() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let _full_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(time_free_trust_packument("acme").to_string())
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    opts.ignore_missing_time_field = true;
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let result = verifier
        .verify(&registry_resolution(), ctx(&"acme".parse::<PkgName>().expect("parse"), "1.1.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn trust_downgrade_still_reported_when_ignored_and_time_is_complete() {
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
    opts.ignore_missing_time_field = true;
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

/// Dropping the missing-time tolerance invalidates a cached run that
/// may have waved entries through on it; adding the tolerance keeps a
/// stricter cached run trustworthy, since it accepted a subset of what
/// today's policy accepts.
#[test]
fn can_trust_past_check_tracks_ignore_missing_time_field() {
    let mut tolerant_opts = default_opts("https://registry.example/");
    tolerant_opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    tolerant_opts.ignore_missing_time_field = true;
    let tolerant = create_npm_resolution_verifier(tolerant_opts);

    let mut strict_opts = default_opts("https://registry.example/");
    strict_opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    let strict = create_npm_resolution_verifier(strict_opts);

    assert!(!strict.can_trust_past_check(tolerant.policy()));
    assert!(tolerant.can_trust_past_check(strict.policy()));
}

/// A record written before the field existed reads as intolerant, which
/// is the safe direction: it cannot have passed anything today's
/// stricter policy would reject.
#[test]
fn can_trust_past_check_reads_a_missing_tolerance_field_as_intolerant() {
    let mut opts = default_opts("https://registry.example/");
    opts.trust_policy = Some(TrustPolicy::NoDowngrade);
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = verifier.policy().clone();
    cached.remove("minimumReleaseAgeIgnoreMissingTime");
    assert!(verifier.can_trust_past_check(&cached));
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

/// Repointing an alias is the change that matters: the alias set is
/// identical, so a digest over alias names alone would still trust the
/// cached policy and reuse resolutions fetched from the old host.
#[test]
fn can_trust_past_check_rejects_changed_named_registry_mapping() {
    let mut opts = default_opts("https://registry.example/");
    opts.registries_by_prefix
        .insert("work".to_string(), "https://registry.work.example/".to_string());
    let verifier = create_npm_resolution_verifier(opts);
    let cached = verifier.policy().clone();
    let mut changed_opts = default_opts("https://registry.example/");
    changed_opts
        .registries_by_prefix
        .insert("work".to_string(), "https://other.example/".to_string());
    let changed = create_npm_resolution_verifier(changed_opts);

    // Pins that the rejection below comes from the URL change and not from
    // something incidental to how the policy is built.
    assert!(verifier.can_trust_past_check(&cached));
    assert!(!changed.can_trust_past_check(&cached));
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
    cached.insert("integrityRequired".to_string(), true.into());
    cached.insert("minimumReleaseAge".to_string(), (60 * 24 * 7).into());
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}

/// Same rule for the missing-integrity check: a record written before
/// the rule existed can't prove it rejected unverifiable tarballs, so
/// the lockfile is re-verified rather than trusted.
#[test]
fn can_trust_past_check_rejects_missing_integrity_required() {
    let mut opts = default_opts("https://registry.example/");
    opts.minimum_release_age = Some(60 * 24);
    let verifier = create_npm_resolution_verifier(opts);

    let mut cached = serde_json::Map::new();
    cached.insert("tarballUrlBinding".to_string(), true.into());
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
    cached.insert("integrityRequired".to_string(), true.into());
    cached.insert("minimumReleaseAge".to_string(), (60 * 24).into()); // past: 1 day
    cached.insert("minimumReleaseAgeExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicy".to_string(), serde_json::Value::Null);
    cached.insert("trustPolicyExclude".to_string(), serde_json::Value::Array(vec![]));
    cached.insert("trustPolicyIgnoreAfter".to_string(), serde_json::Value::Null);
    assert!(!verifier.can_trust_past_check(&cached));
}

/// Any drift in the exclude list invalidates the cached run, even
/// when the drift would have been more permissive (an extra entry):
/// the check is a stricter-than-necessary identity comparison.
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
    cached.insert("integrityRequired".to_string(), true.into());
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
    cached.insert("integrityRequired".to_string(), true.into());
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
    cached.insert("integrityRequired".to_string(), true.into());
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
        revision: None,
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

/// A 403 on the metadata fetch (e.g. a CI token that is authenticated but not
/// authorized to read a private package) must not be reported as a lockfile
/// tarball-URL mismatch: the lockfile is correct, the fetch is the problem. The
/// verifier propagates the registry's own fetch error so the install aborts.
#[tokio::test]
async fn propagates_metadata_fetch_failure_instead_of_a_tampering_mismatch() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let _meta_mock = server
        .mock("GET", "/private-pkg")
        .with_status(403)
        .with_body(r#"{"error":"Forbidden"}"#)
        .create_async()
        .await;

    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{server_url}/private-pkg/-/private-pkg-1.0.0.tgz"),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "private-pkg".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;

    // A transport failure aborts via FetchFailed, never a tampering-style
    // TARBALL_URL_MISMATCH.
    let ResolutionVerification::FetchFailed { message } = result else {
        panic!("expected FetchFailed, got {result:?}");
    };
    assert!(message.contains("403"), "message: {message}");
}

/// The metadata fetch succeeds but does not list the pinned version. That is a
/// genuine verification failure (not a transport error), so it stays
/// `TARBALL_URL_MISMATCH` rather than aborting via `FetchFailed`.
#[tokio::test]
async fn version_absent_from_fetched_metadata_stays_tarball_url_mismatch() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let packument = serde_json::json!({
        "name": "present-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "present-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("{server_url}/present-pkg/-/present-pkg-1.0.0.tgz"),
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/present-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;

    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{server_url}/present-pkg/-/present-pkg-2.0.0.tgz"),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "present-pkg".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "2.0.0")).await;

    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TARBALL_URL_MISMATCH");
}

/// With `registrySupportsTimeField`, a version's publish timestamp is
/// taken from the `time` map of the abbreviated document the verifier
/// already fetched. The `modified` shortcut cannot answer here (the
/// package was modified inside the cutoff window), and no other source
/// is mocked, so a passing verification proves the timestamp came from
/// the abbreviated document — the attestation round-trip and the
/// full-packument download never happen.
#[tokio::test]
async fn registry_supports_time_field_reads_version_time_from_abbreviated_meta() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let now = Utc::now();
    let meta = serde_json::json!({
        "name": "aged-pkg",
        "dist-tags": { "latest": "1.0.0" },
        // Package touched *inside* the cutoff window: the modified
        // shortcut must fall through.
        "modified": now.to_rfc3339(),
        "time": {
            "created": "2020-01-01T00:00:00.000Z",
            "modified": now.to_rfc3339(),
            // The pinned version itself is years old.
            "1.0.0": "2020-01-01T00:00:00.000Z"
        },
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
        // The whole point: one abbreviated fetch answers everything. A
        // second hit would be the full-packument fallback this flag
        // exists to avoid.
        .expect(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24); // 24h
    opts.registry_supports_time_field = true;
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
