use super::{FastOverrideOptions, try_fast_update_overrides};
use indexmap::IndexMap;
use pnpm_config_parse_overrides::{PackageSelector, VersionOverride};
use pnpm_lockfile::{Lockfile, LockfileResolution, PkgName, SnapshotEntry, TarballResolution};
use pnpm_resolving_resolver_base::{
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
                    revision: None,
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

async fn try_update(
    lockfile: &Lockfile,
    parsed_overrides: &[VersionOverride],
    resolved_overrides: &IndexMap<String, String>,
    resolver: &dyn Resolver,
) -> Option<Lockfile> {
    let resolve_options = ResolveOptions::default();
    let registries =
        HashMap::from([("default".to_string(), "https://registry.npmjs.org/".to_string())]);
    try_fast_update_overrides(FastOverrideOptions {
        context: super::RewriteContext {
            lockfile,
            resolver,
            resolve_options: &resolve_options,
            manifest_hook: None,
            registries: &registries,
            registry_options_by_url: &std::collections::BTreeMap::new(),
            lockfile_include_tarball_url: false,
        },
        parsed_overrides,
        resolved_overrides,
    })
    .await
}

async fn update_with_manifest(manifest: serde_json::Value) -> (Option<Lockfile>, usize) {
    let lockfile = lockfile();
    let parsed = parsed_override();
    let overrides = IndexMap::from([("target".to_string(), "2.0.0".to_string())]);
    let resolver = StubResolver { calls: AtomicUsize::new(0), manifest };
    let result = try_update(&lockfile, &parsed, &overrides, &resolver).await;
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

#[tokio::test]
async fn falls_back_when_registry_metadata_has_invalid_dependency_fields() {
    for manifest in [
        json!({
            "name": "target",
            "version": "2.0.0",
            "peerDependencies": ""
        }),
        json!({
            "name": "target",
            "version": "2.0.0",
            "peerDependenciesMeta": []
        }),
        json!({
            "name": "target",
            "version": "2.0.0",
            "engines": "node"
        }),
    ] {
        let (updated, calls) = update_with_manifest(manifest).await;
        assert_eq!(calls, 1);
        assert!(updated.is_none());
    }
}

#[tokio::test]
async fn drops_obsolete_dependency_edges_from_a_replacement() {
    let (updated, calls) = update_with_manifest(json!({
        "name": "target",
        "version": "2.0.0"
    }))
    .await;
    let updated = updated.expect("fast override update");
    let target = updated
        .snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&"target@2.0.0".parse().unwrap()))
        .expect("target snapshot");

    assert_eq!(calls, 1);
    assert!(target.dependencies.is_none());
    assert!(
        updated
            .snapshots
            .as_ref()
            .is_some_and(|snapshots| { !snapshots.contains_key(&"child@1.1.0".parse().unwrap()) }),
    );
}

#[tokio::test]
async fn reuses_a_unique_compatible_locked_dependency_added_by_a_replacement() {
    let mut lockfile = lockfile();
    let importer = lockfile.importers.get_mut(".").expect("root importer");
    importer.dependencies.as_mut().expect("dependencies").insert(
        PkgName::parse("added").unwrap(),
        serde_json::from_value(json!({
            "specifier": "1.0.0",
            "version": "1.0.0"
        }))
        .unwrap(),
    );
    lockfile.packages.as_mut().expect("packages").insert(
        "added@1.0.0".parse().unwrap(),
        serde_json::from_value(json!({
            "resolution": {
                "integrity": "sha512-added"
            }
        }))
        .unwrap(),
    );
    lockfile
        .snapshots
        .as_mut()
        .expect("snapshots")
        .insert("added@1.0.0".parse().unwrap(), SnapshotEntry::default());
    let parsed = parsed_override();
    let overrides = IndexMap::from([("target".to_string(), "2.0.0".to_string())]);
    let resolver = StubResolver {
        calls: AtomicUsize::new(0),
        manifest: json!({
            "name": "target",
            "version": "2.0.0",
            "dependencies": {
                "child": "^1.0.0",
                "added": "^1.0.0"
            }
        }),
    };

    let updated =
        try_update(&lockfile, &parsed, &overrides, &resolver).await.expect("fast override update");
    let target = updated
        .snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&"target@2.0.0".parse().unwrap()))
        .expect("target snapshot");

    assert_eq!(resolver.calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        target
            .dependencies
            .as_ref()
            .and_then(|dependencies| dependencies.get(&PkgName::parse("added").unwrap()))
            .map(ToString::to_string)
            .as_deref(),
        Some("1.0.0"),
    );
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
    let result = try_update(lockfile, &parsed, &overrides, &resolver).await;
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

#[tokio::test]
async fn removes_a_dependency_only_from_matching_parent_snapshots() {
    let mut lockfile = lockfile();
    lockfile.overrides = None;
    let parsed = vec![VersionOverride {
        selector: "parent@^1>target".to_string(),
        parent_pkg: Some(PackageSelector {
            name: "parent".to_string(),
            bare_specifier: Some("^1".to_string()),
        }),
        target_pkg: PackageSelector { name: "target".to_string(), bare_specifier: None },
        new_bare_specifier: "-".to_string(),
        converge: false,
    }];
    let overrides = IndexMap::from([("parent@^1>target".to_string(), "-".to_string())]);
    let resolver = StubResolver {
        calls: AtomicUsize::new(0),
        manifest: json!({
            "name": "target",
            "version": "2.0.0"
        }),
    };

    let updated = try_update(&lockfile, &parsed, &overrides, &resolver)
        .await
        .expect("fast dependency removal");
    let parent = updated
        .snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&"parent@1.0.0".parse().unwrap()))
        .expect("parent snapshot");

    assert_eq!(resolver.calls.load(Ordering::Relaxed), 0);
    assert!(parent.dependencies.is_none());
}

#[tokio::test]
async fn applies_exact_replacements_and_dependency_removals_together() {
    let mut lockfile = lockfile();
    lockfile.packages.as_mut().expect("packages").insert(
        "obsolete@1.0.0".parse().unwrap(),
        serde_json::from_value(json!({
            "resolution": {
                "integrity": "sha512-obsolete"
            }
        }))
        .unwrap(),
    );
    lockfile
        .snapshots
        .as_mut()
        .expect("snapshots")
        .insert("obsolete@1.0.0".parse().unwrap(), SnapshotEntry::default());
    lockfile
        .snapshots
        .as_mut()
        .and_then(|snapshots| snapshots.get_mut(&"parent@1.0.0".parse().unwrap()))
        .and_then(|parent| parent.dependencies.as_mut())
        .expect("parent dependencies")
        .insert(PkgName::parse("obsolete").unwrap(), "1.0.0".parse().unwrap());
    let parsed = vec![
        parsed_override().remove(0),
        VersionOverride {
            selector: "obsolete".to_string(),
            parent_pkg: None,
            target_pkg: PackageSelector { name: "obsolete".to_string(), bare_specifier: None },
            new_bare_specifier: "-".to_string(),
            converge: false,
        },
    ];
    let overrides = IndexMap::from([
        ("target".to_string(), "2.0.0".to_string()),
        ("obsolete".to_string(), "-".to_string()),
    ]);
    let resolver = StubResolver {
        calls: AtomicUsize::new(0),
        manifest: json!({
            "name": "target",
            "version": "2.0.0",
            "dependencies": {
                "child": "^1.0.0",
                "obsolete": "^1.0.0"
            }
        }),
    };

    let updated = try_update(&lockfile, &parsed, &overrides, &resolver)
        .await
        .expect("fast mixed override update");
    let parent = updated
        .snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&"parent@1.0.0".parse().unwrap()))
        .expect("parent snapshot");

    assert_eq!(resolver.calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        parent
            .dependencies
            .as_ref()
            .and_then(|dependencies| dependencies.get(&PkgName::parse("target").unwrap()))
            .map(ToString::to_string)
            .as_deref(),
        Some("2.0.0"),
    );
    assert!(parent.dependencies.as_ref().is_none_or(|dependencies| {
        !dependencies.contains_key(&PkgName::parse("obsolete").unwrap())
    }));
    let replacement = updated
        .snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&"target@2.0.0".parse().unwrap()))
        .expect("replacement snapshot");
    assert!(replacement.dependencies.as_ref().is_none_or(|dependencies| {
        !dependencies.contains_key(&PkgName::parse("obsolete").unwrap())
    }));
    assert!(
        updated
            .snapshots
            .as_ref()
            .is_some_and(|snapshots| !snapshots.contains_key(&"obsolete@1.0.0".parse().unwrap())),
    );
}

/// The same graph plus a second, higher version of `target` that another
/// importer holds. The registry has 2.0.0 too, which nothing locks.
fn lockfile_with_two_target_versions() -> Lockfile {
    let mut lockfile = lockfile();
    // No override recorded yet: one at 1.0.0 could not coexist with an
    // importer holding 1.5.0, since it would have moved that edge too.
    lockfile.overrides = None;
    lockfile.importers.insert(
        "pkg-a".to_string(),
        serde_json::from_value(json!({
            "dependencies": { "target": { "specifier": "1.5.0", "version": "1.5.0" } }
        }))
        .expect("importer"),
    );
    lockfile.packages.as_mut().expect("packages").insert(
        "target@1.5.0".parse().expect("package key"),
        serde_json::from_value(json!({ "resolution": { "integrity": "sha512-target-2" } }))
            .expect("package"),
    );
    lockfile.snapshots.as_mut().expect("snapshots").insert(
        "target@1.5.0".parse().expect("snapshot key"),
        serde_json::from_value(json!({})).expect("snapshot"),
    );
    lockfile
}

fn range_override(value: &str) -> Vec<VersionOverride> {
    vec![VersionOverride {
        selector: "target".to_string(),
        parent_pkg: None,
        target_pkg: PackageSelector { name: "target".to_string(), bare_specifier: None },
        new_bare_specifier: value.to_string(),
        converge: false,
    }]
}

/// Records the version each resolution asks for. The rewrite that follows
/// is the pre-existing machinery; what this pins is the version the range
/// maps to.
struct RecordingResolver {
    requested: std::sync::Mutex<Vec<String>>,
}

impl Resolver for RecordingResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        self.requested
            .lock()
            .expect("record the requested version")
            .push(wanted_dependency.bare_specifier.clone().expect("version"));
        Box::pin(async { Ok(None) })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

#[tokio::test]
async fn a_range_override_moves_to_the_version_the_graph_already_holds() {
    let lockfile = lockfile_with_two_target_versions();
    let overrides = IndexMap::from([("target".to_string(), "^1.2.0".to_string())]);
    let resolver = RecordingResolver { requested: std::sync::Mutex::new(Vec::new()) };

    // The rewrite itself stops at the stub's empty resolution; the version
    // it asked for is the decision under test.
    let _ = try_update(&lockfile, &range_override("^1.2.0"), &overrides, &resolver).await;

    assert_eq!(
        *resolver.requested.lock().expect("requested versions"),
        vec!["1.5.0".to_string()],
        "resolution prefers the locked 1.5.0 over the registry's higher versions",
    );
}

#[tokio::test]
async fn a_range_override_no_locked_version_satisfies_falls_back() {
    let lockfile = lockfile_with_two_target_versions();
    let overrides = IndexMap::from([("target".to_string(), "^3.0.0".to_string())]);
    let resolver = StubResolver { calls: AtomicUsize::new(0), manifest: json!({}) };

    assert!(
        try_update(&lockfile, &range_override("^3.0.0"), &overrides, &resolver).await.is_none(),
        "only the resolver can fetch a version the lockfile does not hold",
    );
    assert_eq!(resolver.calls.load(Ordering::Relaxed), 0, "and it bails before resolving");
}

/// `parent` and `other` both depend on `target@1.0.0`, so a selector
/// naming only `parent` has to leave `other` where it is.
fn lockfile_with_two_dependents() -> Lockfile {
    let mut lockfile = lockfile();
    lockfile.overrides = None;
    lockfile.packages.as_mut().expect("packages").insert(
        "other@1.0.0".parse().expect("package key"),
        serde_json::from_value(json!({ "resolution": { "integrity": "sha512-other" } }))
            .expect("package"),
    );
    lockfile.snapshots.as_mut().expect("snapshots").insert(
        "other@1.0.0".parse().expect("snapshot key"),
        serde_json::from_value(json!({ "dependencies": { "target": "1.0.0" } })).expect("snapshot"),
    );
    lockfile
        .importers
        .get_mut(".")
        .expect("importer")
        .dependencies
        .as_mut()
        .expect("dependencies")
        .insert(
            "other".parse().expect("alias"),
            serde_json::from_value(json!({ "specifier": "1.0.0", "version": "1.0.0" }))
                .expect("dependency"),
        );
    lockfile
}

fn parent_scoped_override() -> Vec<VersionOverride> {
    vec![VersionOverride {
        selector: "parent>target".to_string(),
        parent_pkg: Some(PackageSelector { name: "parent".to_string(), bare_specifier: None }),
        target_pkg: PackageSelector { name: "target".to_string(), bare_specifier: None },
        new_bare_specifier: "2.0.0".to_string(),
        converge: false,
    }]
}

#[tokio::test]
async fn a_parent_scoped_override_moves_only_that_parents_edge() {
    let lockfile = lockfile_with_two_dependents();
    let overrides = IndexMap::from([("parent>target".to_string(), "2.0.0".to_string())]);
    let resolver = StubResolver {
        calls: AtomicUsize::new(0),
        manifest: json!({ "name": "target", "version": "2.0.0", "dependencies": { "child": "^1.0.0" } }),
    };

    let updated = try_update(&lockfile, &parent_scoped_override(), &overrides, &resolver)
        .await
        .expect("moving one parent's edge needs no resolution");

    let target: PkgName = "target".parse().expect("package name");
    let edge_of = |owner: &str| {
        updated
            .snapshots
            .as_ref()
            .and_then(|snapshots| snapshots.get(&owner.parse().expect("snapshot key")))
            .and_then(|snapshot| snapshot.dependencies.as_ref())
            .and_then(|dependencies| dependencies.get(&target))
            .map(ToString::to_string)
    };
    assert_eq!(edge_of("parent@1.0.0").as_deref(), Some("2.0.0"), "the named parent moves");
    assert_eq!(edge_of("other@1.0.0").as_deref(), Some("1.0.0"), "the other dependent does not");
    let mut keys: Vec<_> = updated
        .snapshots
        .as_ref()
        .expect("snapshots")
        .keys()
        .map(ToString::to_string)
        .filter(|key| key.starts_with("target@"))
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["target@1.0.0".to_string(), "target@2.0.0".to_string()],
        "both versions survive, one per dependent",
    );
}

#[tokio::test]
async fn a_parent_scoped_override_prunes_the_old_version_when_nothing_else_holds_it() {
    let mut lockfile = lockfile_with_two_dependents();
    // `other` no longer reaches `target`, so the named parent is its last
    // dependent and the version it leaves becomes unreachable.
    lockfile
        .snapshots
        .as_mut()
        .expect("snapshots")
        .get_mut(&"other@1.0.0".parse().expect("snapshot key"))
        .expect("other snapshot")
        .dependencies = None;
    let overrides = IndexMap::from([("parent>target".to_string(), "2.0.0".to_string())]);
    let resolver = StubResolver {
        calls: AtomicUsize::new(0),
        manifest: json!({ "name": "target", "version": "2.0.0", "dependencies": { "child": "^1.0.0" } }),
    };

    let updated = try_update(&lockfile, &parent_scoped_override(), &overrides, &resolver)
        .await
        .expect("moving one parent's edge needs no resolution");

    let keys: Vec<_> = updated
        .snapshots
        .as_ref()
        .expect("snapshots")
        .keys()
        .map(ToString::to_string)
        .filter(|key| key.starts_with("target@"))
        .collect();
    assert_eq!(keys, vec!["target@2.0.0".to_string()], "the version it left is unreachable");
}
