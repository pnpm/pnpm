use std::{collections::HashMap, str::FromStr, sync::Mutex};

use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_resolver_base::{
    EXISTING_VERSION_SELECTOR_WEIGHT, LatestQuery, PreferredVersions, ResolveError, ResolveFuture,
    ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, VersionSelectorEntry,
    VersionSelectorType, VersionSelectorWithWeight, VersionSelectors, WantedDependency,
};
use pretty_assertions::assert_eq;

// `import_granularity` wants the two `resolve_importer` entries collapsed to
// `resolve_importer::{self, ..}`, but `crate::resolve_importer` is both a
// module (the nested items below) and a re-exported function (the bare entry);
// `self` would only re-import the module, dropping the function. The tree is
// already minimal, so suppress the false positive.
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::import_granularity,
        reason = "`resolve_importer` is both a module and a re-exported fn; the value- and type-namespace entries cannot be merged via `self`"
    )
)]
use crate::{
    DepPath, ResolveDependencyTreeError, resolve_importer,
    resolve_importer::{ResolveImporterError, ResolveImporterOptions},
};

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
        manifest: Some(std::sync::Arc::new(manifest)),
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

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn fake_manifest(root_deps: serde_json::Value) -> (tempfile::TempDir, PackageManifest) {
    fake_manifest_json(serde_json::json!({
        "name": "root",
        "version": "0.0.0",
        "dependencies": root_deps,
    }))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn fake_manifest_json(json: serde_json::Value) -> (tempfile::TempDir, PackageManifest) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("package.json");
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("parse package.json");
    (tmp, manifest)
}

fn default_opts() -> ResolveImporterOptions {
    ResolveImporterOptions {
        auto_install_peers: true,
        auto_install_peers_from_highest_match: false,
        resolve_peers_from_workspace_root: false,
        dedupe_peers: false,
        all_preferred_versions: PreferredVersions::new(),
        patched_dependencies: None,
        base_opts: ResolveOptions::default(),
        pick_lowest_direct: false,
        subdep_published_by: None,
        catalogs: pacquet_catalogs_types::Catalogs::new(),
        exclude_links_from_lockfile: false,
        lockfile_dir: None,
        modules_dir: None,
        peers_suffix_max_length: 1000,
        catalog_server: false,
        manifest_hook: None,
        pnpmfile_hook: None,
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
        "react should no longer be missing after hoisting",
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

    #[expect(
        clippy::needless_collect,
        reason = "Collecting into a Vec keeps the assertion readable; `.any(...)` on the iterator would be denser without saving meaningful work."
    )]
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
        "transitive peer should be hoisted to importer direct deps: {direct:?}",
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

// ---------------------------------------------------------------------------
// Ports of upstream's deps-installer `autoInstallPeers.ts` test cases. Each
// covers a single-importer scenario from
// <https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-installer/test/install/autoInstallPeers.ts>
// ---------------------------------------------------------------------------

/// Port of "auto install non-optional peer dependencies": only the
/// required peer is hoisted; optional peers without a preferred-version
/// hint stay missing. Mirrors the lockfile snapshot
/// `[abc-optional-peers(peer-a), peer-a]` upstream asserts on.
#[tokio::test]
async fn auto_install_skips_optional_peers_without_preferred_versions() {
    let mut table = HashMap::new();
    table.insert(
        ("abc".to_string(), "1.0.0".to_string()),
        fake_result(
            "abc",
            "1.0.0",
            serde_json::json!({
                "name": "abc",
                "version": "1.0.0",
                "peerDependencies": {
                    "peer-a": "^1.0.0",
                    "peer-b": "^1.0.0",
                    "peer-c": "^1.0.0",
                },
                "peerDependenciesMeta": {
                    "peer-b": { "optional": true },
                    "peer-c": { "optional": true },
                },
            }),
        ),
    );
    table.insert(
        ("peer-a".to_string(), "^1.0.0".to_string()),
        fake_result("peer-a", "1.0.0", serde_json::json!({ "name": "peer-a", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "abc": "1.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"peer-a"), "required peer should be hoisted: {direct:?}");
    assert!(
        !direct.contains(&"peer-b"),
        "optional peer must stay missing without a preferred version",
    );
    assert!(
        !direct.contains(&"peer-c"),
        "optional peer must stay missing without a preferred version",
    );
}

/// A locked optional peer version is preserved on re-resolution. The
/// optional peer `peer-c` is recorded in the preferred versions twice: a
/// plain entry for the lower `1.0.0` a sibling workspace package declares
/// directly, and a weighted entry for the already-locked higher `1.0.1`
/// seeded from the wanted lockfile. Optional peer hoisting must consider
/// the weighted entry too — otherwise the locked `1.0.1` is discarded and
/// the lockfile is rewritten to the sibling's `1.0.0`. Regression test for
/// <https://github.com/pnpm/pnpm/pull/12075>; the end-to-end equivalent
/// lives in pnpm's `autoInstallPeers.ts`.
#[tokio::test]
async fn keeps_locked_optional_peer_over_lower_sibling_version() {
    let mut table = HashMap::new();
    table.insert(
        ("abc".to_string(), "1.0.0".to_string()),
        fake_result(
            "abc",
            "1.0.0",
            serde_json::json!({
                "name": "abc",
                "version": "1.0.0",
                "peerDependencies": {
                    "peer-a": "^1.0.0",
                    "peer-c": "^1.0.0",
                },
                "peerDependenciesMeta": {
                    "peer-c": { "optional": true },
                },
            }),
        ),
    );
    table.insert(
        ("peer-a".to_string(), "^1.0.0".to_string()),
        fake_result("peer-a", "1.0.0", serde_json::json!({ "name": "peer-a", "version": "1.0.0" })),
    );
    for version in ["1.0.0", "1.0.1"] {
        table.insert(
            ("peer-c".to_string(), version.to_string()),
            fake_result(
                "peer-c",
                version,
                serde_json::json!({ "name": "peer-c", "version": version }),
            ),
        );
    }
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "abc": "1.0.0" }));

    let mut opts = default_opts();
    let mut peer_c_selectors = VersionSelectors::new();
    peer_c_selectors
        .insert("1.0.0".to_string(), VersionSelectorEntry::Plain(VersionSelectorType::Version));
    peer_c_selectors.insert(
        "1.0.1".to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: EXISTING_VERSION_SELECTOR_WEIGHT,
        }),
    );
    opts.all_preferred_versions.insert("peer-c".to_string(), peer_c_selectors);

    let result =
        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();

    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("peer-c"),
        Some(&DepPath::from("peer-c@1.0.1".to_string())),
        "the already-locked optional peer 1.0.1 must win over the sibling's 1.0.0",
    );
    let abc = result
        .peers_result
        .direct_dependencies_by_alias
        .get("abc")
        .expect("abc resolved")
        .to_string();
    assert!(abc.contains("(peer-c@1.0.1)"), "abc should keep the locked optional peer: {abc}");
    assert!(
        !abc.contains("(peer-c@1.0.0)"),
        "abc must not adopt the sibling's lower version: {abc}",
    );
}

/// Port of "auto install the common peer dependency": two consumers
/// each declare a peer-c range that share an exact-version intersection
/// (`1` and `1.0.0`). The single intersected pick lands in the tree.
#[tokio::test]
async fn auto_install_dedupes_via_range_intersection_when_identical() {
    let mut table = HashMap::new();
    table.insert(
        ("wants-peer-c-1".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-peer-c-1",
            "1.0.0",
            serde_json::json!({
                "name": "wants-peer-c-1",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("wants-peer-c-1.0.0".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-peer-c-1.0.0",
            "1.0.0",
            serde_json::json!({
                "name": "wants-peer-c-1.0.0",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("peer-c".to_string(), "1.0.0".to_string()),
        fake_result("peer-c", "1.0.0", serde_json::json!({ "name": "peer-c", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "wants-peer-c-1": "1.0.0",
        "wants-peer-c-1.0.0": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"peer-c"), "single intersected peer-c should be hoisted: {direct:?}");
    // Exactly one peer-c@1.0.0 in the graph — the two consumers share it.
    let peer_c_entries: Vec<&DepPath> = result
        .peers_result
        .graph
        .keys()
        .filter(|dp| dp.to_string().starts_with("peer-c@"))
        .collect();
    assert_eq!(peer_c_entries.len(), 1, "expected one peer-c entry, got: {peer_c_entries:?}");
}

/// Port of "do not auto install when there is no common peer dependency
/// range intersection": with `autoInstallPeersFromHighestMatch: false`
/// the picker drops the peer when the ranges don't reduce to one
/// unique string. The consumers stay pure (no peer suffix).
#[tokio::test]
async fn auto_install_does_not_install_when_no_intersection() {
    let mut table = HashMap::new();
    table.insert(
        ("wants-peer-c-1".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-peer-c-1",
            "1.0.0",
            serde_json::json!({
                "name": "wants-peer-c-1",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("wants-peer-c-2".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-peer-c-2",
            "1.0.0",
            serde_json::json!({
                "name": "wants-peer-c-2",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "2.0.0" },
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "wants-peer-c-1": "1.0.0",
        "wants-peer-c-2": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(!direct.contains(&"peer-c"), "peer-c must not be hoisted on conflict: {direct:?}");
}

/// Port of "auto install latest when there is no common peer dependency
/// range intersection": same setup as above but with
/// `autoInstallPeersFromHighestMatch: true`, the picker joins the
/// ranges with `||` and the resolver picks a satisfying version.
#[tokio::test]
async fn auto_install_from_highest_match_installs_on_conflict() {
    let mut table = HashMap::new();
    table.insert(
        ("wants-peer-c-1".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-peer-c-1",
            "1.0.0",
            serde_json::json!({
                "name": "wants-peer-c-1",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("wants-peer-c-2".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-peer-c-2",
            "1.0.0",
            serde_json::json!({
                "name": "wants-peer-c-2",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "2.0.0" },
            }),
        ),
    );
    table.insert(
        ("peer-c".to_string(), "1.0.0 || 2.0.0".to_string()),
        fake_result("peer-c", "2.0.0", serde_json::json!({ "name": "peer-c", "version": "2.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "wants-peer-c-1": "1.0.0",
        "wants-peer-c-2": "1.0.0",
    }));

    let mut opts = default_opts();
    opts.auto_install_peers_from_highest_match = true;
    let result =
        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"peer-c"), "peer-c should land via `||` join: {direct:?}");
}

/// Port of "hoist a peer dependency in order to reuse it by other
/// dependencies, when it satisfies them": a sibling that already
/// brings the peer's exact version into scope is reused by the
/// hoist-picker via preferred-versions, so we don't re-resolve.
#[tokio::test]
async fn auto_install_reuses_peer_already_brought_by_a_sibling() {
    let mut table = HashMap::new();
    table.insert(
        ("xyz-parent".to_string(), "1.0.0".to_string()),
        fake_result(
            "xyz-parent",
            "1.0.0",
            serde_json::json!({
                "name": "xyz-parent",
                "version": "1.0.0",
                "dependencies": { "xyz": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("xyz".to_string(), "1.0.0".to_string()),
        fake_result(
            "xyz",
            "1.0.0",
            serde_json::json!({
                "name": "xyz",
                "version": "1.0.0",
                "peerDependencies": { "x": "^1.0.0", "y": "^1.0.0", "z": "^1.0.0" },
            }),
        ),
    );
    table.insert(
        ("xyz-with-xyz".to_string(), "1.0.0".to_string()),
        fake_result(
            "xyz-with-xyz",
            "1.0.0",
            serde_json::json!({
                "name": "xyz-with-xyz",
                "version": "1.0.0",
                "dependencies": { "xyz": "1.0.0", "x": "1.0.0", "y": "1.0.0", "z": "1.0.0" },
            }),
        ),
    );
    for name in ["x", "y", "z"] {
        table.insert(
            (name.to_string(), "1.0.0".to_string()),
            fake_result(name, "1.0.0", serde_json::json!({ "name": name, "version": "1.0.0" })),
        );
    }
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "xyz-parent": "1.0.0",
        "xyz-with-xyz": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    for name in ["x", "y", "z"] {
        assert!(direct.contains(&name), "{name} should be hoisted to importer: {direct:?}");
    }
    // The sibling already supplies x@1.0.0 / y@1.0.0 / z@1.0.0, so the
    // hoist-picker must reuse that exact version via preferred-versions
    // — never the peer's `^1.0.0` range arm. (The resolver may still be
    // called multiple times with the same `1.0.0` spec because the
    // tree walker doesn't gate the `resolve()` call on dedup; what
    // matters here is that `^1.0.0` never appears.)
    let calls = resolver.calls.lock().unwrap();
    for name in ["x", "y", "z"] {
        let ranges: Vec<&str> = calls
            .iter()
            .filter(|(call_name, _)| call_name == name)
            .map(|(_, range)| range.as_str())
            .collect();
        assert!(
            ranges.iter().all(|range| *range == "1.0.0"),
            "{name} should resolve via the sibling's exact-version spec only, got {ranges:?}",
        );
    }
}

/// Port of "don't auto-install a peer dependency, when that dependency
/// is in the root": a direct dep at the importer level satisfies the
/// peer, so the hoist-picker doesn't add a fresh entry.
#[tokio::test]
async fn auto_install_does_not_hoist_when_root_already_has_dep() {
    let mut table = HashMap::new();
    table.insert(
        ("xyz".to_string(), "1.0.0".to_string()),
        fake_result(
            "xyz",
            "1.0.0",
            serde_json::json!({
                "name": "xyz",
                "version": "1.0.0",
                "peerDependencies": { "x": "^1.0.0" },
            }),
        ),
    );
    table.insert(
        ("x".to_string(), "1.0.0".to_string()),
        fake_result("x", "1.0.0", serde_json::json!({ "name": "x", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "xyz": "1.0.0",
        "x": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    // The picker must not re-resolve `x` via the peer's `^1.0.0` range
    // when the root already pinned it at `1.0.0`.
    let calls = resolver.calls.lock().unwrap();
    let x_ranges: Vec<String> =
        calls.iter().filter(|(n, _)| n == "x").map(|(_, r)| r.clone()).collect();
    assert_eq!(
        x_ranges,
        vec!["1.0.0".to_string()],
        "`x` should resolve only via the importer's direct spec, got: {x_ranges:?}",
    );
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("xyz"),
        Some(&DepPath::from("xyz@1.0.0(x@1.0.0)".to_string())),
    );
}

/// An *optional* peer that is only available deep in the freshly
/// resolved tree (brought by a sibling, not pinned in the wanted
/// lockfile) must NOT be hoisted, so the consumer's depPath stays bare.
/// `getHoistableOptionalPeers` reads upstream's static
/// `ctx.allPreferredVersions` (lockfile + manifests, empty on a fresh
/// install) — never the run-resolved versions. Feeding the latter in
/// would resolve the optional peer where pnpm leaves it unresolved, and
/// non-deterministically at that (the result would depend on resolution
/// order). Regression test for <https://github.com/pnpm/pnpm/issues/12266>.
#[tokio::test]
async fn optional_peer_only_in_resolved_tree_is_not_hoisted() {
    let mut table = HashMap::new();
    table.insert(
        ("needs-opt".to_string(), "1.0.0".to_string()),
        fake_result(
            "needs-opt",
            "1.0.0",
            serde_json::json!({
                "name": "needs-opt",
                "version": "1.0.0",
                "peerDependencies": { "opt": "^1.0.0" },
                "peerDependenciesMeta": { "opt": { "optional": true } },
            }),
        ),
    );
    // `provider` pulls `opt@1.0.0` deep in the tree — a sibling of
    // `needs-opt`, not an ancestor, so `opt` is out of `needs-opt`'s
    // resolution scope.
    table.insert(
        ("provider".to_string(), "1.0.0".to_string()),
        fake_result(
            "provider",
            "1.0.0",
            serde_json::json!({
                "name": "provider",
                "version": "1.0.0",
                "dependencies": { "opt": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("opt".to_string(), "1.0.0".to_string()),
        fake_result("opt", "1.0.0", serde_json::json!({ "name": "opt", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "needs-opt": "1.0.0",
        "provider": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    // `opt` must not be hoisted to the importer, and `needs-opt`'s
    // optional peer must stay unresolved — bare `needs-opt@1.0.0`.
    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(!direct.contains(&"opt"), "optional peer `opt` must not be hoisted: {direct:?}");
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("needs-opt"),
        Some(&DepPath::from("needs-opt@1.0.0".to_string())),
    );
}

/// Port of "don't install the same missing peer dependency twice": a
/// transitive chain where each layer adds the same peer must produce a
/// single hoisted entry.
#[tokio::test]
async fn auto_install_does_not_install_same_missing_peer_twice() {
    let mut table = HashMap::new();
    table.insert(
        ("outer".to_string(), "1.0.0".to_string()),
        fake_result(
            "outer",
            "1.0.0",
            serde_json::json!({
                "name": "outer",
                "version": "1.0.0",
                "dependencies": { "inner": "1.0.0" },
                "peerDependencies": { "y": "^1.0.0" },
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
                "peerDependencies": { "y": "^1.0.0" },
            }),
        ),
    );
    table.insert(
        ("y".to_string(), "^1.0.0".to_string()),
        fake_result("y", "1.0.0", serde_json::json!({ "name": "y", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "outer": "1.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let y_entries: Vec<&DepPath> =
        result.peers_result.graph.keys().filter(|dp| dp.to_string().starts_with("y@")).collect();
    assert_eq!(y_entries.len(), 1, "expected one y entry, got: {y_entries:?}");
    let calls = resolver.calls.lock().unwrap();
    let y_calls = calls.iter().filter(|(n, _)| n == "y").count();
    assert_eq!(y_calls, 1, "y should be resolved at most once");
}

/// Port of "prefer the peer dependency version already used in the
/// root": when the importer declares the peer itself, its pinned
/// version wins via the importer-peerDependencies seed (matching
/// upstream's `getAllDependenciesFromManifest({ autoInstallPeers })`)
/// — even if `latest` would resolve higher.
#[tokio::test]
async fn auto_install_prefers_peer_version_pinned_in_importer_peerdeps() {
    let mut table = HashMap::new();
    table.insert(
        ("has-y-peer".to_string(), "1.0.0".to_string()),
        fake_result(
            "has-y-peer",
            "1.0.0",
            serde_json::json!({
                "name": "has-y-peer",
                "version": "1.0.0",
                "peerDependencies": { "y": ">=1.0.0" },
            }),
        ),
    );
    // The importer pinned `y: ^1.0.0` so the resolver only sees that
    // spec — never `>=1.0.0` (the peer range). Were the importer
    // peerDeps not walked, the picker would fall back to the peer
    // range and might pick y@2.0.0.
    table.insert(
        ("y".to_string(), "^1.0.0".to_string()),
        fake_result("y", "1.0.0", serde_json::json!({ "name": "y", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest_json(serde_json::json!({
        "name": "root",
        "version": "0.0.0",
        "peerDependencies": {
            "has-y-peer": "1.0.0",
            "y": "^1.0.0",
        },
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"y"), "importer's own peer dep should land as direct: {direct:?}");
    assert!(direct.contains(&"has-y-peer"));
    let calls = resolver.calls.lock().unwrap();
    let y_ranges: Vec<String> =
        calls.iter().filter(|(n, _)| n == "y").map(|(_, r)| r.clone()).collect();
    assert_eq!(
        y_ranges,
        vec!["^1.0.0".to_string()],
        "y should resolve via importer's spec only, got: {y_ranges:?}",
    );
}

/// Port of "auto install hoisted peer dependency": when the same peer
/// name is brought into the graph by a regular `dependencies` edge of
/// one consumer (at an exact version) and as a peer of another, the
/// regular-dep version wins via the preferred-versions table.
#[tokio::test]
async fn auto_install_hoisted_peer_dep_reuses_regular_dep_version() {
    let mut table = HashMap::new();
    table.insert(
        ("has-c-in-deps".to_string(), "1.0.0".to_string()),
        fake_result(
            "has-c-in-deps",
            "1.0.0",
            serde_json::json!({
                "name": "has-c-in-deps",
                "version": "1.0.0",
                "dependencies": { "c": "2.0.0" },
            }),
        ),
    );
    table.insert(
        ("wants-c".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-c",
            "1.0.0",
            serde_json::json!({
                "name": "wants-c",
                "version": "1.0.0",
                "peerDependencies": { "c": "^2.0.0" },
            }),
        ),
    );
    table.insert(
        ("c".to_string(), "2.0.0".to_string()),
        fake_result("c", "2.0.0", serde_json::json!({ "name": "c", "version": "2.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "has-c-in-deps": "1.0.0",
        "wants-c": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let c_entries: Vec<String> = result
        .peers_result
        .graph
        .keys()
        .map(ToString::to_string)
        .filter(|dp| dp.starts_with("c@"))
        .collect();
    assert_eq!(
        c_entries,
        vec!["c@2.0.0".to_string()],
        "expected one c@2.0.0 entry (not a second copy via the peer arm), got: {c_entries:?}",
    );
}

/// `catalog:` on a direct dependency is rewritten to the catalog's
/// recorded specifier before the resolver chain sees the wanted dep.
/// Mirrors upstream's
/// [importer-only catalog dereference](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/installing/deps-resolver/src/resolveDependencies.ts#L592-L611).
#[tokio::test]
async fn catalog_protocol_on_direct_dep_is_rewritten() {
    let mut table = HashMap::new();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result("foo", "1.2.0", serde_json::json!({ "name": "foo", "version": "1.2.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "catalog:" }));

    let mut catalogs = pacquet_catalogs_types::Catalogs::new();
    catalogs.insert(
        "default".to_string(),
        std::iter::once(("foo".to_string(), "^1.0.0".to_string())).collect(),
    );

    let opts = ResolveImporterOptions { catalogs, ..default_opts() };
    let result =
        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();
    assert_eq!(result.resolved_tree.direct.len(), 1);
    assert_eq!(result.resolved_tree.direct[0].alias, "foo");
    // The resolver chain only sees the catalog-rewritten range.
    let calls = resolver.calls.lock().unwrap();
    assert_eq!(&*calls, &[("foo".to_string(), "^1.0.0".to_string())]);
}

/// A misconfigured `catalog:` entry (here: missing alias) short-
/// circuits resolution with the upstream `CATALOG_ENTRY_NOT_FOUND_FOR_SPEC`
/// error rather than falling through to `SpecNotSupported`.
#[tokio::test]
async fn catalog_misconfiguration_surfaces_pnpm_error_code() {
    let resolver = StubResolver { table: HashMap::new(), calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "catalog:" }));

    let err = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .expect_err("missing catalog entry must error");
    match err {
        ResolveImporterError::Resolve(ResolveDependencyTreeError::CatalogMisconfiguration(
            inner,
        )) => {
            assert_eq!(
                inner.to_string(),
                "No catalog entry 'foo' was found for catalog 'default'.",
            );
        }
        other @ ResolveImporterError::Resolve(_) => {
            panic!("expected CatalogMisconfiguration, got {other:?}")
        }
    }
}

/// Build a [`ResolveResult`] for an `npm:`-aliased install. `local_alias`
/// is the alias the importer uses in `node_modules/` (and in
/// `parentPkgs`); `real_name`/`version` identify the resolved package.
/// Mirrors the real npm-resolver's behaviour at
/// [`npm_resolver.rs:288`](https://github.com/pnpm/pnpm/blob/2a0032edc0/pacquet/crates/resolving-npm-resolver/src/npm_resolver.rs#L288):
/// the result carries the local alias, while `name_ver` and `id` point
/// at the underlying package.
fn aliased_fake_result(
    local_alias: &str,
    real_name: &str,
    version: &str,
    manifest: serde_json::Value,
) -> ResolveResult {
    let mut result = fake_result(real_name, version, manifest);
    result.alias = Some(local_alias.to_string());
    result
}

/// Regression test for <https://github.com/pnpm/pnpm/issues/11999>.
///
/// The TypeScript fix (`installing/deps-resolver/src/resolvePeers.ts`)
/// broadens which cycles `calculateDepPath` short-circuits. Pacquet's
/// `resolve_peers` walks synchronously with an `in_progress` set, so
/// the deadlock that hit pnpm does not occur here — but the scenario
/// has to keep terminating with a graph entry for the aliased root and
/// for each pair of mutually-peer-depending leaves.
///
/// Layout (from the upstream bug): an aliased install `a@npm:a-real`
/// pulls in `b-real` and `c-real`. Each of those depends on one half
/// of a mutual-peer pair (`x` ↔ `y`) and peer-depends on the aliased
/// root (`a@npm:a-real`). The hoist loop auto-installs `x` and `y` at
/// the importer level, where their cycle surfaces.
#[tokio::test]
async fn aliased_install_with_transitive_mutual_peer_cycle_terminates() {
    let mut table = HashMap::new();
    // Root install: `a@npm:a-real@1.0.0`.
    table.insert(
        ("a".to_string(), "npm:a-real@1.0.0".to_string()),
        aliased_fake_result(
            "a",
            "a-real",
            "1.0.0",
            serde_json::json!({
                "name": "a-real",
                "version": "1.0.0",
                "dependencies": {
                    "b": "npm:b-real@1.0.0",
                    "c": "npm:c-real@1.0.0",
                },
            }),
        ),
    );
    // `b@npm:b-real@1.0.0`: depends on `x`, peer-depends on the aliased
    // root.
    table.insert(
        ("b".to_string(), "npm:b-real@1.0.0".to_string()),
        aliased_fake_result(
            "b",
            "b-real",
            "1.0.0",
            serde_json::json!({
                "name": "b-real",
                "version": "1.0.0",
                "dependencies": { "x": "1.0.0" },
                "peerDependencies": { "a": "npm:a-real@1.0.0" },
            }),
        ),
    );
    // `c@npm:c-real@1.0.0`: depends on `y`, peer-depends on the aliased
    // root.
    table.insert(
        ("c".to_string(), "npm:c-real@1.0.0".to_string()),
        aliased_fake_result(
            "c",
            "c-real",
            "1.0.0",
            serde_json::json!({
                "name": "c-real",
                "version": "1.0.0",
                "dependencies": { "y": "1.0.0" },
                "peerDependencies": { "a": "npm:a-real@1.0.0" },
            }),
        ),
    );
    // `x` ↔ `y` mutual peer cycle.
    table.insert(
        ("x".to_string(), "1.0.0".to_string()),
        fake_result(
            "x",
            "1.0.0",
            serde_json::json!({
                "name": "x",
                "version": "1.0.0",
                "peerDependencies": { "y": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("y".to_string(), "1.0.0".to_string()),
        fake_result(
            "y",
            "1.0.0",
            serde_json::json!({
                "name": "y",
                "version": "1.0.0",
                "peerDependencies": { "x": "1.0.0" },
            }),
        ),
    );

    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "a": "npm:a-real@1.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"a"), "aliased root must surface as a direct dep: {direct:?}");
    assert!(direct.contains(&"x"), "missing peer x must be auto-installed: {direct:?}");
    assert!(direct.contains(&"y"), "missing peer y must be auto-installed: {direct:?}");

    // The aliased root's dep path resolves to the real package id.
    let a_dep_path = result
        .peers_result
        .direct_dependencies_by_alias
        .get("a")
        .expect("alias `a` must be in the result")
        .to_string();
    assert!(
        a_dep_path.starts_with("a-real@1.0.0"),
        "aliased dep path must start with the real package id, got {a_dep_path}",
    );

    // Both mutually-peer-depending leaves land in the graph.
    let dep_paths: std::collections::HashSet<String> =
        result.peers_result.graph.keys().map(ToString::to_string).collect();
    assert!(
        dep_paths.iter().any(|dp| dp.starts_with("x@1.0.0")),
        "x must appear in the graph: {dep_paths:?}",
    );
    assert!(
        dep_paths.iter().any(|dp| dp.starts_with("y@1.0.0")),
        "y must appear in the graph: {dep_paths:?}",
    );
}

/// `resolutionMode` orchestration tests: assert the deps-resolver hands
/// the npm resolver the right per-depth [`ResolveOptions`]
/// (`pick_lowest_version`, `published_by`) for each mode. These cover
/// the wiring in [`TreeCtx::with_resolution_mode`] +
/// [`resolve_node`](crate::resolve_dependency_tree); the version pick
/// itself lives in the npm picker (tested there).
mod resolution_mode {
    use super::{StubResolver, default_opts, fake_manifest, fake_result};
    use crate::resolve_importer;
    use chrono::{DateTime, TimeZone, Utc};
    use pacquet_package_manifest::DependencyGroup;
    use pacquet_resolving_resolver_base::{
        ResolveFuture, ResolveOptions, ResolveResult, Resolver, WantedDependency,
    };
    use pretty_assertions::assert_eq;
    use std::{collections::HashMap, sync::Mutex};

    /// The `(pick_lowest_version, published_by)` pair recorded per alias.
    type RecordedOpts = (bool, Option<DateTime<Utc>>);

    /// Resolver that records the [`RecordedOpts`] each `(alias, range)`
    /// query was resolved with, so a test can assert the depth-specific
    /// options the tree walker built.
    struct RecordingResolver {
        inner: StubResolver,
        seen: Mutex<HashMap<String, RecordedOpts>>,
    }

    impl RecordingResolver {
        fn new(table: HashMap<(String, String), ResolveResult>) -> Self {
            RecordingResolver {
                inner: StubResolver { table, calls: Mutex::new(Vec::new()) },
                seen: Mutex::new(HashMap::new()),
            }
        }

        fn opts_for(&self, alias: &str) -> RecordedOpts {
            *self.seen.lock().unwrap().get(alias).expect("alias was resolved")
        }
    }

    impl Resolver for RecordingResolver {
        fn resolve<'a>(
            &'a self,
            wanted: &'a WantedDependency,
            opts: &'a ResolveOptions,
        ) -> ResolveFuture<'a> {
            if let Some(alias) = wanted.alias.clone() {
                self.seen
                    .lock()
                    .unwrap()
                    .insert(alias, (opts.pick_lowest_version, opts.published_by));
            }
            self.inner.resolve(wanted, opts)
        }

        fn resolve_latest<'a>(
            &'a self,
            query: &'a pacquet_resolving_resolver_base::LatestQuery,
            opts: &'a ResolveOptions,
        ) -> pacquet_resolving_resolver_base::ResolveLatestFuture<'a> {
            self.inner.resolve_latest(query, opts)
        }
    }

    fn one_dep_one_subdep_table() -> HashMap<(String, String), ResolveResult> {
        let mut table = HashMap::new();
        table.insert(
            ("direct".to_string(), "^1.0.0".to_string()),
            fake_result(
                "direct",
                "1.0.0",
                serde_json::json!({
                    "name": "direct",
                    "version": "1.0.0",
                    "dependencies": { "sub": "^2.0.0" }
                }),
            ),
        );
        table.insert(
            ("sub".to_string(), "^2.0.0".to_string()),
            fake_result("sub", "2.0.0", serde_json::json!({ "name": "sub", "version": "2.0.0" })),
        );
        table
    }

    /// `highest` (the default): both direct and transitive deps are
    /// picked highest, with the same `minimumReleaseAge` cutoff applied
    /// uniformly.
    #[tokio::test]
    async fn highest_mode_picks_highest_everywhere() {
        let resolver = RecordingResolver::new(one_dep_one_subdep_table());
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "direct": "^1.0.0" }));
        let maximum = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let mut opts = default_opts();
        opts.base_opts.published_by = Some(maximum);
        opts.pick_lowest_direct = false;
        opts.subdep_published_by = Some(maximum);

        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();

        assert_eq!(resolver.opts_for("direct"), (false, Some(maximum)));
        assert_eq!(resolver.opts_for("sub"), (false, Some(maximum)));
    }

    /// `lowest-direct`: direct deps pick lowest, transitive deps pick
    /// highest, and there is no extra publish-date cutoff beyond
    /// `minimumReleaseAge` (here unset).
    #[tokio::test]
    async fn lowest_direct_mode_picks_lowest_only_for_direct_deps() {
        let resolver = RecordingResolver::new(one_dep_one_subdep_table());
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "direct": "^1.0.0" }));
        let mut opts = default_opts();
        opts.pick_lowest_direct = true;
        opts.subdep_published_by = None;

        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();

        assert_eq!(resolver.opts_for("direct"), (true, None));
        assert_eq!(resolver.opts_for("sub"), (false, None));
    }

    /// `time-based`: direct deps pick lowest under the
    /// `minimumReleaseAge` cutoff; transitive deps pick highest but are
    /// constrained to the computed publish-date cutoff. The cutoff
    /// itself is computed workspace-wide in `resolve_workspace`; here we
    /// pass it in directly to assert the depth-specific threading.
    #[tokio::test]
    async fn time_based_mode_threads_cutoff_to_subdeps_only() {
        let resolver = RecordingResolver::new(one_dep_one_subdep_table());
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "direct": "^1.0.0" }));
        let maximum = Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap();
        let cutoff = Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).unwrap();
        let mut opts = default_opts();
        opts.base_opts.published_by = Some(maximum);
        opts.pick_lowest_direct = true;
        opts.subdep_published_by = Some(cutoff);

        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();

        assert_eq!(resolver.opts_for("direct"), (true, Some(maximum)));
        assert_eq!(resolver.opts_for("sub"), (false, Some(cutoff)));
    }
}
