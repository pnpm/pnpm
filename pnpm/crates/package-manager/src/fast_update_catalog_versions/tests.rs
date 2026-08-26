use super::try_fast_update_catalog_versions;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::{Lockfile, LockfileResolution, TarballResolution};
use pnpm_resolving_resolver_base::{
    LatestInfo, LatestQuery, PkgResolutionId, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

struct StubResolver {
    manifest: Value,
}

impl Resolver for StubResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
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

/// `target` is a direct dependency of the importer, reached only through
/// the default catalog.
fn lockfile() -> Lockfile {
    serde_json::from_value(json!({
        "lockfileVersion": "9.0",
        "catalogs": {
            "default": {
                "target": { "specifier": "1.0.0", "version": "1.0.0" }
            }
        },
        "importers": {
            ".": {
                "dependencies": {
                    "target": { "specifier": "catalog:", "version": "1.0.0" }
                }
            }
        },
        "packages": {
            "target@1.0.0": { "resolution": { "integrity": "sha512-target-1" } },
            "child@1.1.0": { "resolution": { "integrity": "sha512-child" } }
        },
        "snapshots": {
            "target@1.0.0": { "dependencies": { "child": "1.1.0" } },
            "child@1.1.0": {}
        }
    }))
    .expect("lockfile")
}

fn catalogs(specifier: &str) -> Catalogs {
    Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([("target".to_string(), specifier.to_string())]),
    )])
}

fn manifest_requiring_child(range: &str) -> Value {
    json!({ "name": "target", "version": "2.0.0", "dependencies": { "child": range } })
}

async fn try_update(lockfile: &Lockfile, catalogs: &Catalogs, manifest: Value) -> Option<Lockfile> {
    let resolve_options = ResolveOptions::default();
    let registries =
        HashMap::from([("default".to_string(), "https://registry.npmjs.org/".to_string())]);
    let resolver = StubResolver { manifest };
    try_fast_update_catalog_versions(
        &crate::fast_update_overrides::RewriteContext {
            lockfile,
            resolver: &resolver,
            resolve_options: &resolve_options,
            manifest_hook: None,
            registries: &registries,
            registry_options_by_url: &std::collections::BTreeMap::new(),
            lockfile_include_tarball_url: false,
        },
        catalogs,
    )
    .await
}

fn snapshot_keys(lockfile: &Lockfile) -> Vec<String> {
    let mut keys: Vec<_> =
        lockfile.snapshots.as_ref().expect("snapshots").keys().map(ToString::to_string).collect();
    keys.sort();
    keys
}

#[tokio::test]
async fn replaces_the_catalog_version_when_the_locked_child_still_fits() {
    let updated = try_update(&lockfile(), &catalogs("2.0.0"), manifest_requiring_child("^1.0.0"))
        .await
        .expect("the locked child satisfies the new manifest");

    assert_eq!(
        snapshot_keys(&updated),
        vec!["child@1.1.0".to_string(), "target@2.0.0".to_string()],
    );
    let entry = &updated.catalogs.as_ref().expect("catalogs")["default"]["target"];
    assert_eq!((entry.specifier.as_str(), entry.version.as_str()), ("2.0.0", "2.0.0"));
    assert_eq!(
        updated.importers["."].dependencies.as_ref().expect("dependencies")
            [&"target".parse().expect("alias")]
            .version
            .to_string(),
        "2.0.0",
        "the importer follows the catalog to the new version",
    );
}

#[tokio::test]
async fn falls_back_when_the_locked_child_does_not_satisfy_the_new_version() {
    assert!(
        try_update(&lockfile(), &catalogs("2.0.0"), manifest_requiring_child("^2.0.0"))
            .await
            .is_none(),
        "resolving the new child is the resolver's job",
    );
}

#[tokio::test]
async fn falls_back_when_an_importer_depends_on_the_package_directly() {
    let mut lockfile = lockfile();
    lockfile.importers.insert(
        "pkg-a".to_string(),
        serde_json::from_value(json!({
            "dependencies": { "target": { "specifier": "1.0.0", "version": "1.0.0" } }
        }))
        .expect("importer"),
    );

    assert!(
        try_update(&lockfile, &catalogs("2.0.0"), manifest_requiring_child("^1.0.0"))
            .await
            .is_none(),
        "that importer pinned the old version, so the graph would need both",
    );
}

#[tokio::test]
async fn falls_back_when_a_package_depends_on_the_catalog_package() {
    let mut lockfile = lockfile();
    lockfile.snapshots.as_mut().expect("snapshots").insert(
        "parent@1.0.0".parse().expect("snapshot key"),
        serde_json::from_value(json!({ "dependencies": { "target": "1.0.0" } })).expect("snapshot"),
    );

    assert!(
        try_update(&lockfile, &catalogs("2.0.0"), manifest_requiring_child("^1.0.0"))
            .await
            .is_none(),
        "the transitive dependent keeps the old version, so both would be needed",
    );
}

#[tokio::test]
async fn falls_back_when_a_catalog_reference_has_no_recorded_entry() {
    let mut subject = lockfile();
    subject.importers.insert(
        "pkg-a".to_string(),
        serde_json::from_value(json!({
            "specifiers": { "other": "catalog:" },
            "dependencies": { "other": { "specifier": "catalog:", "version": "1.0.0" } }
        }))
        .expect("importer"),
    );

    assert!(
        try_update(&subject, &catalogs("2.0.0"), manifest_requiring_child("^1.0.0"))
            .await
            .is_none(),
        "that importer's catalog entry was never recorded, so the graph is incomplete",
    );
}

#[tokio::test]
async fn falls_back_when_two_catalogs_move_the_same_alias() {
    let mut subject = lockfile();
    subject.catalogs.as_mut().expect("catalogs").insert(
        "other".to_string(),
        serde_json::from_value(json!({
            "target": { "specifier": "1.0.0", "version": "1.0.0" }
        }))
        .expect("catalog"),
    );
    let catalogs = Catalogs::from([
        ("default".to_string(), BTreeMap::from([("target".to_string(), "2.0.0".to_string())])),
        ("other".to_string(), BTreeMap::from([("target".to_string(), "3.0.0".to_string())])),
    ]);

    assert!(
        try_update(&subject, &catalogs, manifest_requiring_child("^1.0.0")).await.is_none(),
        "no single catalog is the sole reference once both name it",
    );
}

#[tokio::test]
async fn absorbs_a_range_only_entry_alongside_an_exact_move() {
    let mut subject = lockfile();
    subject.catalogs.as_mut().expect("catalogs").get_mut("default").expect("default").insert(
        "child".to_string(),
        serde_json::from_value(json!({ "specifier": "1.1.0", "version": "1.1.0" }))
            .expect("catalog entry"),
    );
    let catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([
            ("target".to_string(), "2.0.0".to_string()),
            ("child".to_string(), "^1.1.0".to_string()),
        ]),
    )]);

    let updated = try_update(&subject, &catalogs, manifest_requiring_child("^1.0.0"))
        .await
        .expect("the range-only entry rides along with the exact move");

    let recorded = &updated.catalogs.as_ref().expect("catalogs")["default"];
    assert_eq!(
        (recorded["child"].specifier.as_str(), recorded["child"].version.as_str()),
        ("^1.1.0", "1.1.0"),
    );
    assert_eq!(recorded["target"].version.as_str(), "2.0.0");
}

#[tokio::test]
async fn declines_a_range_the_locked_version_still_satisfies() {
    assert!(
        try_update(&lockfile(), &catalogs("^1.0.0"), manifest_requiring_child("^1.0.0"))
            .await
            .is_none(),
        "that change rewrites nothing and belongs to the range-only path",
    );
}
