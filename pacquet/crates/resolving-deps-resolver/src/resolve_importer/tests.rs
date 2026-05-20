use std::{collections::HashMap, str::FromStr, sync::Mutex};

use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_resolver_base::{
    LatestQuery, PreferredVersions, ResolveError, ResolveFuture, ResolveLatestFuture,
    ResolveOptions, ResolveResult, Resolver, WantedDependency,
};
use pretty_assertions::assert_eq;

use crate::{DepPath, resolve_importer, resolve_importer::ResolveImporterOptions};

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
    let name_ver = PkgNameVer::new(
        PkgName::parse(name).unwrap(),
        node_semver::Version::from_str(version).unwrap(),
    );
    ResolveResult {
        id: (&name_ver).into(),
        name_ver: Some(name_ver),
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

fn default_opts() -> ResolveImporterOptions {
    ResolveImporterOptions {
        auto_install_peers: true,
        auto_install_peers_from_highest_match: false,
        resolve_peers_from_workspace_root: false,
        all_preferred_versions: PreferredVersions::new(),
        base_opts: ResolveOptions::default(),
    }
}

#[tokio::test]
async fn auto_installs_missing_required_peer() {
    let mut table = HashMap::new();
    table.insert(
        ("react-dom".to_string(), "18.0.0".to_string()),
        fake_result(
            "react-dom",
            "18.0.0",
            serde_json::json!({
                "name": "react-dom",
                "version": "18.0.0",
                "peerDependencies": { "react": "^18.0.0" }
            }),
        ),
    );
    // When hoistPeers proposes react, it'll come in as the missing
    // peer's wanted range — "^18.0.0".
    table.insert(
        ("react".to_string(), "^18.0.0".to_string()),
        fake_result("react", "18.2.0", serde_json::json!({ "name": "react", "version": "18.2.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "react-dom": "18.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct_aliases: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct_aliases.contains(&"react"), "react should be hoisted: {direct_aliases:?}");
    assert!(direct_aliases.contains(&"react-dom"));
    // Peer is resolved — the issue list is empty for react.
    assert!(
        !result.peers_result.peer_dependency_issues.missing.contains_key("react"),
        "react should no longer be missing after hoisting"
    );
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("react-dom"),
        Some(&DepPath::from("react-dom@18.0.0(react@18.2.0)".to_string())),
    );
}

#[tokio::test]
async fn does_not_hoist_when_disabled() {
    let mut table = HashMap::new();
    table.insert(
        ("react-dom".to_string(), "18.0.0".to_string()),
        fake_result(
            "react-dom",
            "18.0.0",
            serde_json::json!({
                "name": "react-dom",
                "version": "18.0.0",
                "peerDependencies": { "react": "^18.0.0" }
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "react-dom": "18.0.0" }));

    let mut opts = default_opts();
    opts.auto_install_peers = false;
    let result =
        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(!direct.contains(&"react"));
    assert!(result.peers_result.peer_dependency_issues.missing.contains_key("react"));
}

#[tokio::test]
async fn transitive_required_peer_is_hoisted() {
    let mut table = HashMap::new();
    table.insert(
        ("outer".to_string(), "1.0.0".to_string()),
        fake_result(
            "outer",
            "1.0.0",
            serde_json::json!({
                "name": "outer",
                "version": "1.0.0",
                "dependencies": { "inner": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("inner".to_string(), "1.0.0".to_string()),
        fake_result(
            "inner",
            "1.0.0",
            serde_json::json!({
                "name": "inner",
                "version": "1.0.0",
                "peerDependencies": { "peer-pkg": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("peer-pkg".to_string(), "^1.0.0".to_string()),
        fake_result(
            "peer-pkg",
            "1.2.3",
            serde_json::json!({ "name": "peer-pkg", "version": "1.2.3" }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "outer": "1.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(
        direct.contains(&"peer-pkg"),
        "transitive peer should be hoisted to importer direct deps: {direct:?}"
    );
    assert!(!result.peers_result.peer_dependency_issues.missing.contains_key("peer-pkg"));
}

#[tokio::test]
async fn reuses_preferred_version_instead_of_resolving_fresh() {
    let mut table = HashMap::new();
    table.insert(
        ("react".to_string(), "18.2.0".to_string()),
        fake_result("react", "18.2.0", serde_json::json!({ "name": "react", "version": "18.2.0" })),
    );
    table.insert(
        ("react-dom".to_string(), "18.0.0".to_string()),
        fake_result(
            "react-dom",
            "18.0.0",
            serde_json::json!({
                "name": "react-dom",
                "version": "18.0.0",
                "peerDependencies": { "react": "^18.0.0" }
            }),
        ),
    );
    // hoistPeers picks the already-resolved 18.2.0 instead of "^18.0.0".
    // The stub returns the same result for both keys so a stray
    // "^18.0.0" resolve call would still work — but the assertion
    // below also checks the call list.
    table.insert(
        ("react".to_string(), "18.2.0".to_string()),
        fake_result("react", "18.2.0", serde_json::json!({ "name": "react", "version": "18.2.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "react": "18.2.0", "react-dom": "18.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let calls = resolver.calls.lock().unwrap();
    // No `react` re-resolve via a different range — the only react
    // request was the direct dep at "18.2.0".
    let react_call_count = calls.iter().filter(|(name, _)| name == "react").count();
    assert_eq!(react_call_count, 1, "should not re-resolve react via a hoisted spec");

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"react"));
    assert!(direct.contains(&"react-dom"));
}
