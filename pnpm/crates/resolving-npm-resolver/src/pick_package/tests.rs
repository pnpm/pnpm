mod behavior;

mod release_age;

mod trust_policy;

mod cache_partitions;

mod cache_read_modes;

mod version_selection;

use std::sync::Arc;

use pnpm_network::{
    AuthHeaders, MetadataCacheScope, RetryOpts, ThrottledClient, UpstreamRouteHook,
};
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use chrono::{DateTime, Utc};
use pnpm_config::version_policy::create_package_version_policy;
use pnpm_resolving_resolver_base::{
    EXISTING_VERSION_SELECTOR_WEIGHT, VersionSelectorEntry, VersionSelectorType,
    VersionSelectorWithWeight, VersionSelectors,
};

use super::{
    InMemoryPackageMetaCache, PickPackageContext, PickPackageError, PickPackageOptions,
    metadata_cache_key, persist_meta_to_mirror, pick_package, shared_packument_fetch_locker,
};
use crate::{
    mirror::{
        ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, get_pkg_mirror_path, load_meta,
    },
    pick_package_from_meta::{RegistryPackageSpec, RegistryPackageSpecType},
    registry_url::to_registry_url,
};

const PACKAGE_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.1.0" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "acme",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/acme-1.0.0.tgz"
            }
        },
        "1.1.0": {
            "name": "acme",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/acme-1.1.0.tgz"
            }
        }
    }
}"#;

/// [`PACKAGE_BODY`] with 1.1.0 left out of the `time` map — the shape
/// registries such as npmmirror serve, where an abbreviated packument
/// reports publish times for only some of the versions it lists.
const PARTIAL_TIME_PACKAGE_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.1.0" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "acme",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/acme-1.0.0.tgz"
            }
        },
        "1.1.0": {
            "name": "acme",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/acme-1.1.0.tgz"
            }
        }
    }
}"#;

/// [`PACKAGE_BODY`] as it looked before 1.1.0 was published — for
/// seeding a mirror that predates a version the registry has.
const STALE_PACKAGE_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.0.0" },
    "modified": "2024-01-15T12:00:00.000Z",
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z"
    },
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

fn range_spec(name: &str, range: &str) -> RegistryPackageSpec {
    RegistryPackageSpec {
        name: name.to_string(),
        fetch_spec: range.to_string(),
        spec_type: RegistryPackageSpecType::Range,
        revision: None,
        normalized_bare_specifier: None,
    }
}

fn version_spec(name: &str, version: &str) -> RegistryPackageSpec {
    RegistryPackageSpec {
        name: name.to_string(),
        fetch_spec: version.to_string(),
        spec_type: RegistryPackageSpecType::Version,
        revision: None,
        normalized_bare_specifier: None,
    }
}

fn default_opts(registry: &str) -> PickPackageOptions<'_> {
    PickPackageOptions {
        registry,
        preferred_version_selectors: None,
        published_by: None,
        published_by_exclude: None,
        pick_lowest_version: false,
        include_latest_tag: false,
        dry_run: false,
        optional: false,
        update_checksums: false,
        trust_policy: None,
        blocked_versions: None,
    }
}

/// Abbreviated metadata body, mirroring what a real npm registry
/// returns under `Accept: application/vnd.npm.install-v1+json`.
/// Missing per-version `time`, no `_npmUser`, no `dist.attestations`
/// — just the picker-relevant shape.
const ABBREVIATED_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.0.0" },
    "modified": "2024-12-01T00:00:00.000Z",
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

fn parse_cutoff(rfc3339: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(rfc3339).expect("parse cutoff").with_timezone(&Utc)
}

/// A route hook that records every `(url, package)` it is asked to
/// classify, so a test can assert a cache-served pick still surfaces its
/// route to a server footprint even though no HTTP request happened.
#[derive(Default)]
struct RouteRecorder {
    routes: std::sync::Mutex<Vec<(String, Option<String>)>>,
}

impl UpstreamRouteHook for RouteRecorder {
    fn authorize(&self, url: &str, package: Option<&str>) -> Option<String> {
        self.routes
            .lock()
            .expect("route recorder poisoned")
            .push((url.to_string(), package.map(str::to_string)));
        None
    }
}

/// A route hook that classifies every fetch into one fixed
/// [`MetadataCacheScope`], so a test can drive the private mirror path
/// without standing up a full pnpr route policy.
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
