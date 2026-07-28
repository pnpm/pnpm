use super::{FastOverrideOptions, try_fast_update_overrides};
use indexmap::IndexMap;
use pacquet_config_parse_overrides::{PackageSelector, VersionOverride};
use pacquet_lockfile::{Lockfile, LockfileResolution, PkgName, TarballResolution};
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, PkgResolutionId, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct StubResolver {
    calls: AtomicUsize,
    manifest: serde_json::Value,
}

impl Resolver for StubResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let name = wanted_dependency.alias.clone().expect("alias");
        let version = wanted_dependency.bare_specifier.clone().expect("version");
        let manifest = Arc::new(self.manifest.clone());
        Box::pin(async move {
            Ok(Some(ResolveResult {
                id: PkgResolutionId::from(format!("{name}@{version}")),
                name_ver: Some(format!("{name}@{version}").parse().expect("name and version")),
                latest: Some(version),
                published_at: None,
                manifest: Some(manifest),
                resolution: LockfileResolution::Tarball(TarballResolution {
                    tarball: "https://registry.npmjs.org/target/-/target-2.0.0.tgz".to_string(),
                    integrity: Some("sha512-dGFyZ2V0LTI=".parse().expect("integrity")),
                    git_hosted: None,
                    path: None,
                }),
                resolved_via: "npm-registry".to_string(),
                normalized_bare_specifier: None,
                alias: Some(name),
                policy_violation: None,
            }))
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(Some(LatestInfo { latest_manifest: None })) })
    }
}

fn lockfile() -> Lockfile {
    serde_json::from_value(json!({
        "lockfileVersion": "9.0",
        "overrides": {
            "target": "1.0.0"
        },
        "importers": {
            ".": {
                "dependencies": {
                    "parent": {
                        "specifier": "1.0.0",
                        "version": "1.0.0"
                    }
                }
            }
        },
        "packages": {
            "parent@1.0.0": {
                "resolution": {
                    "integrity": "sha512-parent"
                }
            },
            "target@1.0.0": {
                "resolution": {
                    "integrity": "sha512-target-1"
                }
            },
            "child@1.1.0": {
                "resolution": {
                    "integrity": "sha512-child"
                }
            }
        },
        "snapshots": {
            "parent@1.0.0": {
                "dependencies": {
                    "target": "1.0.0"
                }
            },
            "target@1.0.0": {
                "dependencies": {
                    "child": "1.1.0"
                }
            },
            "child@1.1.0": {}
        }
    }))
    .expect("lockfile")
}

fn parsed_override() -> Vec<VersionOverride> {
    vec![VersionOverride {
        selector: "target".to_string(),
        parent_pkg: None,
        target_pkg: PackageSelector { name: "target".to_string(), bare_specifier: None },
        new_bare_specifier: "2.0.0".to_string(),
        converge: false,
    }]
}

async fn update_with_manifest(manifest: serde_json::Value) -> (Option<Lockfile>, usize) {
    let lockfile = lockfile();
    let parsed = parsed_override();
    let overrides = IndexMap::from([("target".to_string(), "2.0.0".to_string())]);
    let resolver = StubResolver { calls: AtomicUsize::new(0), manifest };
    let result = try_fast_update_overrides(FastOverrideOptions {
        lockfile: &lockfile,
        parsed_overrides: &parsed,
        resolved_overrides: &overrides,
        resolver: &resolver,
        resolve_options: &ResolveOptions::default(),
        manifest_hook: None,
        registries: &std::collections::HashMap::from([(
            "default".to_string(),
            "https://registry.npmjs.org/".to_string(),
        )]),
        lockfile_include_tarball_url: false,
    })
    .await;
    (result, resolver.calls.load(Ordering::Relaxed))
}

#[tokio::test]
async fn rewrites_an_exact_override_when_locked_children_satisfy_the_new_manifest() {
    let (updated, calls) = update_with_manifest(json!({
        "name": "target",
        "version": "2.0.0",
        "dependencies": {
            "child": "^1.0.0"
        }
    }))
    .await;
    let updated = updated.expect("fast override update");
    let target = PkgName::parse("target").expect("package name");
    let parent_key = "parent@1.0.0".parse().expect("parent key");
    let parent = updated
        .snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&parent_key))
        .expect("parent snapshot");

    assert_eq!(calls, 1);
    assert_eq!(
        parent
            .dependencies
            .as_ref()
            .and_then(|dependencies| dependencies.get(&target))
            .map(ToString::to_string)
            .as_deref(),
        Some("2.0.0"),
    );
    assert!(
        updated
            .snapshots
            .as_ref()
            .is_some_and(|snapshots| snapshots.contains_key(&"target@2.0.0".parse().unwrap())),
    );
}

#[tokio::test]
async fn falls_back_when_a_locked_child_does_not_satisfy_the_new_manifest() {
    let (updated, calls) = update_with_manifest(json!({
        "name": "target",
        "version": "2.0.0",
        "dependencies": {
            "child": "^2.0.0"
        }
    }))
    .await;

    assert_eq!(calls, 1);
    assert!(updated.is_none());
}

async fn remove_target(lockfile: &Lockfile) -> (Option<Lockfile>, usize) {
    let parsed = vec![VersionOverride {
        selector: "target".to_string(),
        parent_pkg: None,
        target_pkg: PackageSelector { name: "target".to_string(), bare_specifier: None },
        new_bare_specifier: "-".to_string(),
        converge: false,
    }];
    let overrides = IndexMap::from([("target".to_string(), "-".to_string())]);
    let resolver = StubResolver {
        calls: AtomicUsize::new(0),
        manifest: json!({
            "name": "target",
            "version": "2.0.0"
        }),
    };
    let result = try_fast_update_overrides(FastOverrideOptions {
        lockfile,
        parsed_overrides: &parsed,
        resolved_overrides: &overrides,
        resolver: &resolver,
        resolve_options: &ResolveOptions::default(),
        manifest_hook: None,
        registries: &std::collections::HashMap::from([(
            "default".to_string(),
            "https://registry.npmjs.org/".to_string(),
        )]),
        lockfile_include_tarball_url: false,
    })
    .await;
    (result, resolver.calls.load(Ordering::Relaxed))
}

#[tokio::test]
async fn removes_a_dependency_and_its_unreachable_subtree_without_resolving() {
    let (updated, calls) = remove_target(&lockfile()).await;
    let updated = updated.expect("fast dependency removal");
    let parent_key = "parent@1.0.0".parse().expect("parent key");
    let parent = updated
        .snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&parent_key))
        .expect("parent snapshot");

    assert_eq!(calls, 0);
    assert!(parent.dependencies.is_none());
    assert!(updated.snapshots.as_ref().is_some_and(|snapshots| {
        !snapshots.contains_key(&"target@1.0.0".parse().unwrap())
            && !snapshots.contains_key(&"child@1.1.0".parse().unwrap())
    }));
    assert!(updated.packages.as_ref().is_some_and(|packages| {
        !packages.contains_key(&"target@1.0.0".parse().unwrap())
            && !packages.contains_key(&"child@1.1.0".parse().unwrap())
    }));
}

#[tokio::test]
async fn falls_back_when_the_removed_dependency_is_used_as_a_peer() {
    let mut lockfile = lockfile();
    lockfile
        .packages
        .as_mut()
        .and_then(|packages| packages.get_mut(&"parent@1.0.0".parse().unwrap()))
        .expect("parent metadata")
        .peer_dependencies = Some(HashMap::from([("target".to_string(), "^1.0.0".to_string())]));

    let (updated, calls) = remove_target(&lockfile).await;

    assert_eq!(calls, 0);
    assert!(updated.is_none());
}
