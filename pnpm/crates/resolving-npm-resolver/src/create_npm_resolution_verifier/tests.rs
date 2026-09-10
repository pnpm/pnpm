mod version_selection;

mod trust_policy;

mod release_age;

mod metadata_cache;

mod behavior;

mod artifact_binding;

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
use pnpm_resolving_resolver_base::{ResolutionVerification, VerifyCtx};
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

const REVISION_ONE_DIGEST: &str =
    "Umd2iCLuYk1I_OFexcp5y9YCy39MIVelFlVpkfIu-Me173sY0f9BxZNw77CFhlHUSpNsEbexRMSP4E3zxqPo2g";
const REVISION_TWO_DIGEST: &str =
    "rMKNsr63tCuqHLAkPUAcy04_zkTXsCh5pSeZqt_1QVItiCJZiy-mZPnVFWwAySSAXXXDhovVbCrLgdN-mONa3A";

fn revision_integrity(digest: &str) -> Integrity {
    format!("sha512-{}==", digest.replace('_', "/").replace('-', "+"))
        .parse()
        .expect("revision integrity")
}

/// Same fixture as [`trust_downgrade_packument`] minus the `time` map:
/// a downgrade the check cannot see because it has no publish order to
/// walk.
fn time_free_trust_packument(name: &str) -> serde_json::Value {
    let mut body = trust_downgrade_packument(name);
    body.as_object_mut().expect("packument is an object").remove("time");
    body
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
