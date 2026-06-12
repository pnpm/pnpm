use std::collections::HashMap;

use node_semver::Version;
use pretty_assertions::assert_eq;

use super::{AuthHeaders, Package, PackageVersion, ThrottledClient};
use crate::package_distribution::PackageDistribution;

#[test]
pub fn package_version_should_include_peers() {
    let mut dependencies = HashMap::<String, String>::new();
    dependencies.insert("fastify".to_string(), "1.0.0".to_string());
    let mut peer_dependencies = HashMap::<String, String>::new();
    peer_dependencies.insert("fast-querystring".to_string(), "1.0.0".to_string());
    let version = PackageVersion {
        name: String::new(),
        version: Version::parse("1.0.0").unwrap(),
        dist: PackageDistribution::default(),
        dependencies: Some(dependencies),
        dev_dependencies: None,
        peer_dependencies: Some(peer_dependencies),
        optional_dependencies: None,
        peer_dependencies_meta: None,
        other: HashMap::default(),
        npm_user: None,
        deprecated: None,
    };

    let dependencies = |peer| version.dependencies(peer).collect::<HashMap<_, _>>();
    assert!(dependencies(false).contains_key("fastify"));
    assert!(!dependencies(false).contains_key("fast-querystring"));
    assert!(dependencies(true).contains_key("fastify"));
    assert!(dependencies(true).contains_key("fast-querystring"));
    assert!(!dependencies(true).contains_key("hello-world"));
}

#[test]
pub fn serialized_according_to_params() {
    let version = PackageVersion {
        name: String::new(),
        version: Version { major: 3, minor: 2, patch: 1, build: vec![], pre_release: vec![] },
        dist: PackageDistribution::default(),
        dependencies: None,
        dev_dependencies: None,
        peer_dependencies: None,
        optional_dependencies: None,
        peer_dependencies_meta: None,
        other: HashMap::default(),
        npm_user: None,
        deprecated: None,
    };

    assert_eq!(version.serialize(true), "3.2.1");
    assert_eq!(version.serialize(false), "^3.2.1");
}

/// [`Package::fetch_from_registry`] must attach the registry-keyed
/// `Authorization` header on every metadata GET, even for the
/// abbreviated install-v1 endpoint. `mockito::Matcher::Exact`
/// rejects the request unless the header arrives verbatim, so a
/// missing or wrong header would 501 the request and propagate as
/// a deserialization error.
#[tokio::test]
async fn fetch_from_registry_attaches_authorization_header() {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{"name":"acme","dist-tags":{"latest":"1.0.0"},"versions":{}}"#;
    let mock = server
        .mock("GET", "/acme")
        .match_header("authorization", "Bearer top-secret")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;

    let registry = format!("{}/", server.url());
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::from_creds_map(
        [(pacquet_network::nerf_dart(&registry), "Bearer top-secret".to_owned())],
        None,
    );

    let pkg = Package::fetch_from_registry("acme", &client, &registry, &auth_headers)
        .await
        .expect("server should accept the request once the bearer header is attached");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;
}

fn package_with_versions(name: &str, versions: &[&str], latest: &str) -> Package {
    let versions_map = versions
        .iter()
        .map(|version| {
            (
                version.to_string(),
                PackageVersion {
                    name: name.to_string(),
                    version: Version::parse(version).unwrap(),
                    dist: PackageDistribution::default(),
                    dependencies: None,
                    dev_dependencies: None,
                    peer_dependencies: None,
                    optional_dependencies: None,
                    peer_dependencies_meta: None,
                    other: HashMap::default(),
                    npm_user: None,
                    deprecated: None,
                },
            )
        })
        .collect();
    let mut dist_tags = HashMap::new();
    dist_tags.insert("latest".to_string(), latest.to_string());
    Package {
        name: name.to_string(),
        dist_tags,
        versions: versions_map,
        time: None,
        modified: None,
        etag: None,
        homepage: None,
        mutex: std::sync::Arc::default(),
    }
}

/// `Package` equality is by `name` only; the mutex and versions
/// `HashMap` (whose iteration order is non-deterministic) are
/// excluded. Two packages with the same name compare equal even
/// when their `versions` maps differ — this lets call sites
/// dedupe in-flight metadata fetches against the package name.
#[test]
fn package_equality_compares_by_name_only() {
    let lhs = package_with_versions("acme", &["1.0.0"], "1.0.0");
    let rhs = package_with_versions("acme", &["2.0.0"], "2.0.0");
    assert_eq!(lhs, rhs);

    let other = package_with_versions("widget", &["1.0.0"], "1.0.0");
    assert_ne!(lhs, other);
}

/// `latest()` looks up the version named under the `latest`
/// dist-tag and returns the corresponding `PackageVersion`.
#[test]
fn latest_returns_version_pointed_to_by_dist_tag() {
    let pkg = package_with_versions("acme", &["1.0.0", "2.0.0", "3.0.0"], "2.0.0");
    let latest = pkg.latest();
    assert_eq!(latest.expect("latest manifest decodes").version.to_string(), "2.0.0");
}

/// `pinned_version` picks the highest version inside the given
/// range, mirroring `node-semver`'s `maxSatisfying`.
#[test]
fn pinned_version_picks_highest_matching() {
    let pkg = package_with_versions("acme", &["1.0.0", "1.2.0", "1.5.3", "2.0.0"], "2.0.0");
    let picked =
        pkg.pinned_version("^1.0.0").expect("at least one 1.x version satisfies the range");
    assert_eq!(picked.version.to_string(), "1.5.3");
}

/// `pinned_version` returns `None` when no version satisfies the
/// range, rather than panicking or falling back to `latest`.
#[test]
fn pinned_version_returns_none_when_no_match() {
    let pkg = package_with_versions("acme", &["1.0.0", "1.2.0"], "1.2.0");
    assert!(pkg.pinned_version("^2.0.0").is_none());
}

/// A real-shape packument carries `time`, `modified`, `_npmUser`,
/// and `dist.attestations.provenance` — the four bits the
/// `minimumReleaseAge` + `trustPolicy='no-downgrade'` verifier
/// consults. All four must round-trip through serde, and the
/// per-version `time` lookup must resolve through
/// [`Package::published_at`].
#[test]
fn package_deserializes_full_provenance_packument() {
    let body = r#"{
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "etag": "\"abc123\"",
        "time": {
            "created": "2025-01-01T00:00:00.000Z",
            "modified": "2025-01-15T12:00:00.000Z",
            "1.0.0": "2025-01-10T08:30:00.000Z"
        },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "_npmUser": {
                    "name": "alice",
                    "email": "alice@example.com",
                    "trustedPublisher": {
                        "id": "github",
                        "oidcConfigId": "release-pipeline"
                    }
                },
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0.tgz",
                    "attestations": {
                        "provenance": { "predicateType": "https://slsa.dev/provenance/v1" },
                        "url": "https://registry/-/npm/v1/attestations/acme@1.0.0"
                    }
                }
            }
        }
    }"#;
    let pkg: Package = serde_json::from_str(body).expect("deserialize full packument");
    assert_eq!(pkg.name, "acme");
    assert_eq!(pkg.modified.as_deref(), Some("2025-01-15T12:00:00.000Z"));
    assert_eq!(pkg.etag.as_deref(), Some(r#""abc123""#));
    assert_eq!(pkg.published_at("1.0.0"), Some("2025-01-10T08:30:00.000Z"));
    assert_eq!(pkg.published_at("9.9.9"), None);

    let version = pkg.versions.get("1.0.0").expect("1.0.0 deserialized");
    let user = version.npm_user.as_ref().expect("_npmUser present");
    let publisher = user.trusted_publisher.as_ref().expect("trustedPublisher present");
    assert_eq!(publisher.id, "github");
    assert_eq!(publisher.oidc_config_id, "release-pipeline");

    let attestations = version.dist.attestations.as_ref().expect("attestations present");
    let provenance = attestations.provenance.as_ref().expect("provenance present");
    assert_eq!(provenance.predicate_type.as_deref(), Some("https://slsa.dev/provenance/v1"));
}

/// A staged publish carries `_npmUser.approver`; that field must
/// round-trip through serde so the trust check can read it as the
/// strongest (`stagedPublish`) evidence.
#[test]
fn package_deserializes_approver_packument() {
    let body = r#"{
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2025-01-10T08:30:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "_npmUser": {
                    "name": "alice",
                    "email": "alice@example.com",
                    "approver": {
                        "name": "bob",
                        "email": "bob@example.com"
                    }
                },
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0.tgz"
                }
            }
        }
    }"#;
    let pkg: Package = serde_json::from_str(body).expect("deserialize approver packument");
    let version = pkg.versions.get("1.0.0").expect("1.0.0 deserialized");
    let user = version.npm_user.as_ref().expect("_npmUser present");
    let approver = user.approver.as_ref().expect("approver present");
    assert_eq!(approver.name.as_deref(), Some("bob"));
    assert_eq!(approver.email.as_deref(), Some("bob@example.com"));
}

/// A packument that doesn't ship `_npmUser` or `attestations` (the
/// common case for older registries) still deserializes; the
/// trust-evidence fields land as `None` and the trust check that
/// reads them treats absence as "no evidence".
#[test]
fn package_deserializes_without_npm_user_or_attestations() {
    let body = r#"{
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2025-01-10T08:30:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0.tgz"
                }
            }
        }
    }"#;
    let pkg: Package = serde_json::from_str(body).expect("deserialize minimal packument");
    let version = pkg.versions.get("1.0.0").expect("1.0.0 deserialized");
    assert!(version.npm_user.is_none(), "missing _npmUser stays None");
    assert!(version.dist.attestations.is_none(), "missing attestations stays None");
    assert!(pkg.modified.is_none(), "missing modified stays None");
    assert!(pkg.etag.is_none(), "missing etag stays None");
}

/// The npm registry sometimes serves `"deprecated": false` (a
/// boolean) on never-deprecated versions even though the upstream
/// type declares the field as a string. JavaScript silently stores
/// the boolean (and the upstream truthiness check happens to do the
/// right thing). Rust serde is strict, so we route through a custom
/// deserializer that normalizes the wire shape: `string` stays a
/// string, `false` becomes `None`, `true` becomes `Some("")`. This
/// regression pinned the entire integrated-benchmark workload, which
/// fails to deserialize `react`, `react-dom`, `scheduler`, and other
/// real-world packages without the normalization.
#[test]
fn package_deserializes_deprecated_boolean_false() {
    let body = r#"{
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "deprecated": false,
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0.tgz"
                }
            }
        }
    }"#;
    let pkg: Package =
        serde_json::from_str(body).expect("deserialize packument with deprecated:false");
    let version = pkg.versions.get("1.0.0").expect("1.0.0 deserialized");
    assert!(version.deprecated.is_none(), "deprecated:false maps to None");
}

#[test]
fn package_deserializes_deprecated_boolean_true() {
    let body = r#"{
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "deprecated": true,
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0.tgz"
                }
            }
        }
    }"#;
    let pkg: Package =
        serde_json::from_str(body).expect("deserialize packument with deprecated:true");
    let version = pkg.versions.get("1.0.0").expect("1.0.0 deserialized");
    assert_eq!(
        version.deprecated.as_deref(),
        Some(""),
        "deprecated:true maps to Some(\"\") — recorded as deprecated without a reason",
    );
}

#[test]
fn package_deserializes_deprecated_reason_string() {
    let body = r#"{
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "deprecated": "use acme@2 instead",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0.tgz"
                }
            }
        }
    }"#;
    let pkg: Package =
        serde_json::from_str(body).expect("deserialize packument with deprecation reason");
    let version = pkg.versions.get("1.0.0").expect("1.0.0 deserialized");
    assert_eq!(version.deprecated.as_deref(), Some("use acme@2 instead"));
}

/// A packument missing the `time` field entirely still
/// deserializes — abbreviated metadata responses omit it, and the
/// verifier falls through to the attestation / full-metadata
/// layers in that case rather than failing closed.
#[test]
fn package_deserializes_without_time_field() {
    let body = r#"{
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "versions": {}
    }"#;
    let pkg: Package = serde_json::from_str(body).expect("deserialize without time");
    assert!(pkg.time.is_none(), "missing time stays None");
    assert!(pkg.published_at("1.0.0").is_none(), "no per-version lookup possible");
}

/// Some historical npm packages (e.g. `deep-diff@0.1.0`) ship
/// `devDependencies` whose values are objects rather than the
/// declared `Record<string, string>` shape. The full packument is
/// fetched by the verifier and includes every version's per-version
/// manifest, so a single malformed historical entry would otherwise
/// fail the whole deserialization and surface as
/// `ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION` for an unrelated
/// version pinned in the lockfile (issue [#11829]). Non-string entries
/// must be silently dropped, matching pnpm's JS-side tolerance.
///
/// [#11829]: https://github.com/pnpm/pnpm/issues/11829
#[test]
fn package_tolerates_object_valued_dependency_entries() {
    let body = r#"{
        "name": "deep-diff",
        "dist-tags": { "latest": "0.3.8" },
        "versions": {
            "0.1.0": {
                "name": "deep-diff",
                "version": "0.1.0",
                "devDependencies": {
                    "vows": {
                        "version": "0.6.4",
                        "dependencies": {
                            "diff": { "version": "1.0.4" },
                            "eyes": { "version": "0.1.8" }
                        }
                    },
                    "should": "1.2.1"
                },
                "dist": {
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/deep-diff-0.1.0.tgz"
                }
            },
            "0.3.8": {
                "name": "deep-diff",
                "version": "0.3.8",
                "dependencies": {},
                "devDependencies": { "mocha": "^2.0.0" },
                "dist": {
                    "shasum": "1111111111111111111111111111111111111111",
                    "tarball": "https://registry/deep-diff-0.3.8.tgz"
                }
            }
        }
    }"#;
    let pkg: Package = serde_json::from_str(body)
        .expect("deserialize packument with object-valued devDependencies entries");

    let old = pkg.versions.get("0.1.0").expect("0.1.0 deserialized");
    let old_dev = old.dev_dependencies.as_ref().expect("devDependencies present");
    assert_eq!(old_dev.get("should").map(String::as_str), Some("1.2.1"));
    assert!(!old_dev.contains_key("vows"), "object-valued entries are dropped");

    let current = pkg.versions.get("0.3.8").expect("0.3.8 deserialized");
    let current_dev = current.dev_dependencies.as_ref().expect("devDependencies present");
    assert_eq!(current_dev.get("mocha").map(String::as_str), Some("^2.0.0"));
}

/// The reserved `time.unpublished` key carries an object value
/// (not a string). [`Package::published_at`] must ignore it
/// instead of returning the object's serialized form. Mirrors the
/// upstream shape at
/// <https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/registry/types/src/index.ts#L20-L25>.
#[test]
fn published_at_skips_reserved_unpublished_object() {
    let body = r#"{
        "name": "acme",
        "dist-tags": {},
        "time": {
            "1.0.0": "2025-01-10T08:30:00.000Z",
            "unpublished": {
                "time": "2025-02-01T00:00:00.000Z",
                "versions": ["0.9.0"]
            }
        },
        "versions": {}
    }"#;
    let pkg: Package = serde_json::from_str(body).expect("deserialize");
    assert_eq!(pkg.published_at("1.0.0"), Some("2025-01-10T08:30:00.000Z"));
    assert_eq!(pkg.published_at("unpublished"), None, "object value isn't a string");
}
