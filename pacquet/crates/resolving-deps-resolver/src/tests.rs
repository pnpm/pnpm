use std::{collections::HashMap, str::FromStr, sync::Mutex};

use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult,
    Resolver, WantedDependency,
};
use pretty_assertions::assert_eq;

use crate::resolve_dependency_tree::{ResolveDependencyTreeOptions, resolve_dependency_tree};

/// Stub resolver fed from a `(name, range)` → `ResolveResult` map.
/// Records each `(name, range)` query so tests can assert dedup.
struct StubResolver {
    table: HashMap<(String, String), ResolveResult>,
    calls: Mutex<Vec<(String, String)>>,
}

impl Resolver for StubResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let key = (
            wanted.alias.clone().unwrap_or_default(),
            wanted.bare_specifier.clone().unwrap_or_default(),
        );
        self.calls.lock().unwrap().push(key.clone());
        let result = self.table.get(&key).cloned();
        Box::pin(async move { Ok::<_, ResolveError>(result) })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

fn fake_result(name: &str, version: &str, manifest: serde_json::Value) -> ResolveResult {
    use pacquet_lockfile::{LockfileResolution, PkgName, PkgNameVer, TarballResolution};
    let id = PkgNameVer::new(
        PkgName::parse(name).unwrap(),
        node_semver::Version::from_str(version).unwrap(),
    );
    ResolveResult {
        id,
        latest: Some(version.to_string()),
        published_at: None,
        manifest: Some(manifest),
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: format!("https://registry.example/{name}-{version}.tgz"),
            integrity: None,
            git_hosted: None,
            path: None,
        }),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: Some(name.to_string()),
        policy_violation: None,
    }
}

fn fake_manifest(root_deps: serde_json::Value) -> (tempfile::TempDir, PackageManifest) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("package.json");
    let json = serde_json::json!({
        "name": "root",
        "version": "0.0.0",
        "dependencies": root_deps,
    });
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("parse package.json");
    (tmp, manifest)
}

#[tokio::test]
async fn walks_dependencies_and_builds_flat_tree() {
    let mut table = HashMap::new();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result(
            "foo",
            "1.2.0",
            serde_json::json!({
                "name": "foo",
                "version": "1.2.0",
                "dependencies": { "bar": "^2.0.0" }
            }),
        ),
    );
    table.insert(
        ("bar".to_string(), "^2.0.0".to_string()),
        fake_result(
            "bar",
            "2.3.0",
            serde_json::json!({
                "name": "bar",
                "version": "2.3.0",
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            auto_install_peers: false,
            base_opts: ResolveOptions::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(tree.direct.len(), 1);
    assert_eq!(tree.direct[0].alias, "foo");
    assert_eq!(tree.direct[0].id, "foo@1.2.0");
    assert_eq!(tree.packages.len(), 2);
    let foo = tree.packages.get("foo@1.2.0").unwrap();
    assert_eq!(foo.children.len(), 1);
    assert_eq!(foo.children[0].id, "bar@2.3.0");
    assert!(tree.policy_violations.is_empty());
}

#[tokio::test]
async fn dedupes_when_the_same_package_appears_in_two_subtrees() {
    let mut table = HashMap::new();
    table.insert(
        ("a".to_string(), "^1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            serde_json::json!({
                "name": "a",
                "version": "1.0.0",
                "dependencies": { "shared": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("b".to_string(), "^1.0.0".to_string()),
        fake_result(
            "b",
            "1.0.0",
            serde_json::json!({
                "name": "b",
                "version": "1.0.0",
                "dependencies": { "shared": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("shared".to_string(), "^1.0.0".to_string()),
        fake_result(
            "shared",
            "1.0.0",
            serde_json::json!({
                "name": "shared",
                "version": "1.0.0",
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "a": "^1.0.0", "b": "^1.0.0" }));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            auto_install_peers: false,
            base_opts: ResolveOptions::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(tree.packages.len(), 3);
    assert!(tree.packages.contains_key("a@1.0.0"));
    assert!(tree.packages.contains_key("b@1.0.0"));
    assert!(tree.packages.contains_key("shared@1.0.0"));
}

#[tokio::test]
async fn declined_specifier_yields_no_direct_entry() {
    let resolver = StubResolver { table: HashMap::new(), calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "git+ssh://example.com" }));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            auto_install_peers: false,
            base_opts: ResolveOptions::default(),
        },
    )
    .await
    .unwrap();

    assert!(tree.direct.is_empty());
    assert!(tree.packages.is_empty());
}
