mod artifact_binding;

mod metadata_cache;

mod trust_policy;

mod workspace_resolution;

mod release_age_jsr;

mod published_versions;
mod release_age;
mod version_guards;
mod workspace_preference;

mod behavior;

mod version_selection;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use pnpm_config::{TrustPolicy, version_policy::create_package_version_policy};
use pnpm_lockfile::{LockfileResolution, RegistryResolution, TarballRevision};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_resolving_resolver_base::{
    CurrentPkg, GuardExhaustionPolicy, LatestQuery, PackageVersionGuard,
    PackageVersionGuardDecision, PackageVersionGuardFuture, PkgResolutionId, ResolveOptions,
    UpdateBehavior, WantedDependency, WorkspacePackage, WorkspacePackages,
    WorkspacePackagesByVersion,
};
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use crate::{
    errors::{
        InvalidRevisionSpecifierError, InvalidTarballIntegrityError, MalformedRevisionHistoryError,
        NoMatchingRevisionError,
    },
    npm_resolver::{NpmResolver, is_not_found_error},
    pick_package::{
        InMemoryPackageMetaCache, shared_packument_fetch_locker, shared_picked_manifest_cache,
    },
    resolve_from_workspace::ResolveFromWorkspaceError,
    violation_codes::MINIMUM_RELEASE_AGE_VIOLATION_CODE,
};

const PACKAGE_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.1.0" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": {
        "1.0.0-canary.1": "2024-01-05T08:30:00.000Z",
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0-canary.1": {
            "name": "acme",
            "version": "1.0.0-canary.1",
            "dist": {
                "integrity": "sha512-EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE==",
                "shasum": "4444444444444444444444444444444444444444",
                "tarball": "https://registry/acme-1.0.0-canary.1.tgz"
            }
        },
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

fn build_resolver(registry: &str) -> (NpmResolver<InMemoryPackageMetaCache>, TempDir) {
    let mut registries = HashMap::new();
    registries.insert("default".to_string(), registry.to_string());
    build_resolver_with_registries(registries)
}

fn build_resolver_with_registries(
    registries: HashMap<String, String>,
) -> (NpmResolver<InMemoryPackageMetaCache>, TempDir) {
    let cache_dir = TempDir::new().expect("tempdir");
    let resolver = NpmResolver {
        registries,
        registries_by_prefix: HashMap::new(),
        http_client: Arc::new(ThrottledClient::default()),
        auth_headers: Arc::new(AuthHeaders::default()),
        meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
        fetch_locker: shared_packument_fetch_locker(),
        picked_manifest_cache: shared_picked_manifest_cache(),
        cache_dir: Some(cache_dir.path().to_path_buf()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    (resolver, cache_dir)
}

#[derive(Debug)]
struct RejectVersions {
    versions: HashSet<String>,
    exhaustion_policy: GuardExhaustionPolicy,
}

impl PackageVersionGuard for RejectVersions {
    fn check<'a>(&'a self, _name: &'a str, version: &'a str) -> PackageVersionGuardFuture<'a> {
        Box::pin(async move {
            if self.versions.contains(version) {
                Ok(PackageVersionGuardDecision::Reject { reason: format!("{version} is blocked") })
            } else {
                Ok(PackageVersionGuardDecision::Allow)
            }
        })
    }

    fn exhaustion_policy(&self) -> GuardExhaustionPolicy {
        self.exhaustion_policy
    }
}

fn guard_rejecting(
    versions: &[&str],
    exhaustion_policy: GuardExhaustionPolicy,
) -> Arc<dyn PackageVersionGuard> {
    Arc::new(RejectVersions {
        versions: versions.iter().map(|version| (*version).to_string()).collect(),
        exhaustion_policy,
    })
}

fn reject_versions(versions: &[&str]) -> Arc<dyn PackageVersionGuard> {
    guard_rejecting(versions, GuardExhaustionPolicy::Fail)
}

/// Packument body for `@jsr/foo__bar` — the npm-shaped name JSR
/// serves `@foo/bar`.
const JSR_PACKAGE_BODY: &str = r#"{
    "name": "@jsr/foo__bar",
    "dist-tags": { "latest": "1.1.0" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "@jsr/foo__bar",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==",
                "shasum": "2222222222222222222222222222222222222222",
                "tarball": "https://registry/foo__bar-1.0.0.tgz"
            }
        },
        "1.1.0": {
            "name": "@jsr/foo__bar",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD==",
                "shasum": "3333333333333333333333333333333333333333",
                "tarball": "https://registry/foo__bar-1.1.0.tgz"
            }
        }
    }
}"#;

/// Packument where the earlier-published `1.0.0` carries the strongest
/// trust evidence available here (`trustedPublisher` + provenance) and
/// the later `1.1.0` carries none — a trust downgrade. Resolving
/// `^1.0.0` picks `1.1.0` (the max), so the resolver-time gate must
/// reject it under `trustPolicy='no-downgrade'`.
const TRUST_DOWNGRADE_PACKAGE_BODY: &str = r#"{
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
            "_npmUser": {
                "name": "alice",
                "trustedPublisher": { "id": "github", "oidcConfigId": "release" }
            },
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/acme-1.0.0.tgz",
                "attestations": {
                    "provenance": { "predicateType": "https://slsa.dev/provenance/v1" }
                }
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

/// Packument with more in-range versions than the guard's re-pick cap, so a
/// guard rejecting all of them stops at the cap rather than at genuine
/// exhaustion.
fn packument_with_many_versions(count: u32) -> String {
    let versions = (0..count)
        .map(|patch| {
            format!(
                r#""1.0.{patch}": {{
            "name": "acme",
            "version": "1.0.{patch}",
            "dist": {{
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/acme-1.0.{patch}.tgz"
            }}
        }}"#,
            )
        })
        .collect::<Vec<_>>()
        .join(",\n        ");
    format!(
        r#"{{
    "name": "acme",
    "dist-tags": {{ "latest": "1.0.{last}" }},
    "modified": "2025-01-15T12:00:00.000Z",
    "versions": {{
        {versions}
    }}
}}"#,
        last = count - 1,
    )
}

/// Packument whose `1.5.0+build` key carries a manifest `version` of
/// `1.5.0` — i.e. the version-map key differs from the parsed manifest
/// version, the case a malformed/malicious registry can produce.
const MISMATCHED_KEY_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.5.0+build" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.5.0+build": "2024-12-10T08:30:00.000Z"
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
        "1.5.0+build": {
            "name": "acme",
            "version": "1.5.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/acme-1.5.0.tgz"
            }
        }
    }
}"#;

/// [`TRUST_DOWNGRADE_PACKAGE_BODY`] as a registry that strips the
/// per-version `time` field serves it.
fn trust_downgrade_body_without_time() -> String {
    let mut body: serde_json::Value =
        serde_json::from_str(TRUST_DOWNGRADE_PACKAGE_BODY).expect("parse fixture packument");
    body.as_object_mut().expect("packument is an object").remove("time");
    body.to_string()
}

fn single_version_body(version: &str, integrity: &str) -> String {
    format!(
        r#"{{
            "name": "acme",
            "dist-tags": {{ "latest": "{version}" }},
            "modified": "2025-01-15T12:00:00.000Z",
            "time": {{ "{version}": "2024-01-10T08:30:00.000Z" }},
            "versions": {{
                "{version}": {{
                    "name": "acme",
                    "version": "{version}",
                    "dist": {{
                        "integrity": "{integrity}",
                        "shasum": "0000000000000000000000000000000000000000",
                        "tarball": "https://registry/acme-{version}.tgz"
                    }}
                }}
            }}
        }}"#,
    )
}

fn build_workspace_packages(name: &str, versions: &[&str]) -> WorkspacePackages {
    let mut by_version: WorkspacePackagesByVersion = BTreeMap::new();
    for version in versions {
        by_version.insert(
            (*version).to_string(),
            WorkspacePackage {
                root_dir: PathBuf::from(format!("/repo/packages/{name}")),
                manifest: json!({ "name": name, "version": version }),
            },
        );
    }
    let mut packages: WorkspacePackages = BTreeMap::new();
    packages.insert(name.to_string(), by_version);
    packages
}

fn workspace_resolve_options(packages: WorkspacePackages) -> ResolveOptions {
    ResolveOptions {
        project_dir: Path::new("/repo/packages/consumer").to_path_buf(),
        lockfile_dir: Path::new("/repo").to_path_buf(),
        workspace_packages: Some(std::sync::Arc::new(packages)),
        link_workspace_packages: pnpm_config::LinkWorkspacePackages::Deep,
        ..ResolveOptions::default()
    }
}

/// Per-version `root_dir`s so the resolved `link:` path identifies
/// which workspace entry the resolver picked.
fn build_workspace_packages_at(name: &str, entries: &[(&str, &str)]) -> WorkspacePackages {
    let mut by_version: WorkspacePackagesByVersion = BTreeMap::new();
    for (version, dir) in entries {
        by_version.insert(
            (*version).to_string(),
            WorkspacePackage {
                root_dir: PathBuf::from(*dir),
                manifest: json!({ "name": name, "version": version }),
            },
        );
    }
    let mut packages: WorkspacePackages = BTreeMap::new();
    packages.insert(name.to_string(), by_version);
    packages
}

/// A packument whose `dist` carries only the legacy `shasum`, the shape
/// behind <https://github.com/pnpm/pnpm/issues/13547>.
fn shasum_only_package_body(shasum: &str) -> String {
    json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "shasum": shasum,
                    "tarball": "https://registry/acme-1.0.0.tgz",
                },
            },
        },
    })
    .to_string()
}

fn revision_package_body(tarball: &str, revision: &serde_json::Value) -> String {
    json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "tarball": tarball,
                    "revision": revision,
                    "revisions": [{
                        "revision": revision,
                        "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                        "tarball": tarball,
                        "manifest": {},
                    }],
                },
            },
        },
    })
    .to_string()
}

fn revision_history_package_body(registry: &str) -> String {
    let digest_a =
        "H0D8ktokFpR1CXnubPWC8tXX0o4YM13gWrxU0FYOD1MChgxlK_CNVgJSql50IQVG82n7u86MEs_HlXsmUv6adQ";
    let digest_b =
        "Umd2iCLuYk1I_OFexcp5y9YCy39MIVelFlVpkfIu-Me173sY0f9BxZNw77CFhlHUSpNsEbexRMSP4E3zxqPo2g";
    let digest_c =
        "rMKNsr63tCuqHLAkPUAcy04_zkTXsCh5pSeZqt_1QVItiCJZiy-mZPnVFWwAySSAXXXDhovVbCrLgdN-mONa3A";
    let integrity_a = format!("sha512-{}==", digest_a.replace('_', "/").replace('-', "+"));
    let integrity_b = format!("sha512-{}==", digest_b.replace('_', "/").replace('-', "+"));
    let integrity_c = format!("sha512-{}==", digest_c.replace('_', "/").replace('-', "+"));
    json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "deprecated": "current warning",
                "dependencies": { "current-only": "1.0.0" },
                "optionalDependencies": { "removed": "1.0.0" },
                "dist": {
                    "integrity": integrity_c,
                    "tarball": format!("{registry}-/tarballs/sha512/{digest_c}"),
                    "revision": 2,
                    "revisions": [
                        {
                            "revision": 0,
                            "integrity": integrity_a,
                            "tarball": format!("{registry}-/tarballs/sha512/{digest_a}"),
                            "manifest": { "dependencies": { "original": "1.0.0" } },
                        },
                        {
                            "revision": 1,
                            "integrity": integrity_b,
                            "tarball": format!("{registry}-/tarballs/sha512/{digest_b}"),
                            "manifest": {
                                "name": "not-acme",
                                "version": "9.0.0",
                                "deprecated": "historical warning",
                                "dist": {},
                                "dependencies": { "fixed": "1.0.0" },
                            },
                        },
                        {
                            "revision": 2,
                            "integrity": integrity_c,
                            "tarball": format!("{registry}-/tarballs/sha512/{digest_c}"),
                            "manifest": { "dependencies": { "selected-current": "1.0.0" } },
                        },
                    ],
                },
            },
        },
    })
    .to_string()
}
