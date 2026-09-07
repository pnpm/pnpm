use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};

use pnpm_lockfile::SnapshotEntry;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::{
    EXISTING_VERSION_SELECTOR_WEIGHT, LatestQuery, PreferredVersions, ResolveError, ResolveFuture,
    ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, VersionSelectorEntry,
    VersionSelectorType, VersionSelectorWithWeight, VersionSelectors, WantedDependency,
};
use pretty_assertions::assert_eq;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use super::{importer_locked_peer_versions, merge_ranges};
use crate::{
    DepPath, ResolveDependencyTreeError, resolve_importer,
    resolve_importer::{ResolveImporterError, ResolveImporterOptions},
};

#[test]
fn locked_peer_versions_are_recorded_for_direct_deps() {
    use pnpm_lockfile::{
        ComVer, ImporterDepVersion, Lockfile, LockfileVersion, PkgName, PkgVerPeer,
        ProjectSnapshot, ResolvedDependencySpec,
    };

    let consumer = PkgName::parse("consumer").unwrap();
    let lockfile = Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).unwrap(),
        importers: std::collections::HashMap::from([(
            "app".to_string(),
            ProjectSnapshot {
                dependencies: Some(std::collections::HashMap::from([(
                    consumer,
                    ResolvedDependencySpec {
                        specifier: "1.0.0".to_string(),
                        version: ImporterDepVersion::Regular(
                            "1.0.0(peer@2.0.0)".parse::<PkgVerPeer>().unwrap(),
                        ),
                    },
                )])),
                ..ProjectSnapshot::default()
            },
        )]),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        packages: None,
        snapshots: None,
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let versions = importer_locked_peer_versions(Some(&lockfile), "app");

    assert_eq!(versions["peer"], HashSet::from_iter(["2.0.0".to_string()]));
}

#[test]
fn only_peer_suffix_versions_are_treated_as_locked_peer_providers() {
    let lockfile = peer_context_lockfile(
        None,
        [
            ("consumer@1.0.0(peer@1.0.0)", SnapshotEntry::default()),
            (
                "other@1.0.0(@types/node@24.0.0)(provider@1.0.0(nested@2.0.0))",
                SnapshotEntry::default(),
            ),
        ],
    );

    assert_eq!(
        locked_peer_names(Some(&lockfile)),
        HashSet::from_iter([
            "@types/node".to_string(),
            "nested".to_string(),
            "peer".to_string(),
            "provider".to_string(),
        ]),
    );
}

#[test]
fn hashed_peer_suffix_uses_package_peer_metadata() {
    let lockfile = peer_context_lockfile(
        Some(("consumer@1.0.0", peer_declaring_metadata(["peer", "missing"]))),
        [(
            "consumer@1.0.0(0123456789abcdef0123456789abcdef)",
            snapshot_with_dependency("peer", plain_dependency("1.0.0")),
        )],
    );

    let versions = importer_locked_peer_versions(Some(&lockfile), "missing-importer");
    assert_eq!(
        versions,
        HashMap::from_iter([("peer".to_string(), HashSet::from_iter(["1.0.0".to_string()]))]),
    );
}

#[test]
fn explicit_peer_suffix_uses_the_dependency_alias() {
    let lockfile = peer_context_lockfile(
        Some(("consumer@1.0.0", peer_declaring_metadata(["peer"]))),
        [(
            "consumer@1.0.0(alias-provider@1.0.0)",
            snapshot_with_dependency("peer", alias_dependency("alias-provider@1.0.0")),
        )],
    );

    let versions = importer_locked_peer_versions(Some(&lockfile), "missing-importer");
    assert_eq!(
        versions,
        HashMap::from_iter([("peer".to_string(), HashSet::from_iter(["1.0.0".to_string()]))]),
    );
}

/// An ordinary dependency may be aliased onto the very package and
/// version a peer resolved to; only the suffix can tell them apart, so
/// the segment keeps the name it spelled.
#[test]
fn an_ordinary_alias_onto_a_peers_provider_does_not_rename_it() {
    let lockfile = peer_context_lockfile(
        Some(("consumer@1.0.0", peer_declaring_metadata(["peer"]))),
        [(
            "consumer@1.0.0(peer@1.0.0)",
            snapshot_with_dependencies([
                ("peer", plain_dependency("1.0.0")),
                ("peer-alias", alias_dependency("peer@1.0.0")),
            ]),
        )],
    );

    let versions = importer_locked_peer_versions(Some(&lockfile), "missing-importer");
    assert_eq!(
        versions,
        HashMap::from_iter([("peer".to_string(), HashSet::from_iter(["1.0.0".to_string()]))]),
    );
}

/// The dependent's `peerDependencies` name the edge a peer resolved
/// through, so a competing ordinary alias onto the same provider does
/// not make the segment unattributable.
#[test]
fn a_declared_peer_alias_outranks_a_competing_ordinary_alias() {
    let lockfile = peer_context_lockfile(
        Some(("consumer@1.0.0", peer_declaring_metadata(["peer"]))),
        [(
            "consumer@1.0.0(alias-provider@1.0.0)",
            snapshot_with_dependencies([
                ("peer", alias_dependency("alias-provider@1.0.0")),
                ("other", alias_dependency("alias-provider@1.0.0")),
            ]),
        )],
    );

    let versions = importer_locked_peer_versions(Some(&lockfile), "missing-importer");
    assert_eq!(
        versions,
        HashMap::from_iter([("peer".to_string(), HashSet::from_iter(["1.0.0".to_string()]))]),
    );
}

/// Two declared peers can share one provider, and the suffix then
/// spells its segment twice — once per peer.
#[test]
fn declared_peers_sharing_a_provider_each_get_their_alias_back() {
    let lockfile = peer_context_lockfile(
        Some(("consumer@1.0.0", peer_declaring_metadata(["first", "second"]))),
        [(
            "consumer@1.0.0(alias-provider@1.0.0)(alias-provider@1.0.0)",
            snapshot_with_dependencies([
                ("first", alias_dependency("alias-provider@1.0.0")),
                ("second", alias_dependency("alias-provider@1.0.0")),
            ]),
        )],
    );

    let versions = importer_locked_peer_versions(Some(&lockfile), "missing-importer");
    assert_eq!(
        versions,
        HashMap::from_iter([
            ("first".to_string(), HashSet::from_iter(["1.0.0".to_string()])),
            ("second".to_string(), HashSet::from_iter(["1.0.0".to_string()])),
        ]),
    );
}

/// A declared peer and a peer propagated from a child can resolve to one
/// provider through separate edges. Taking the declared edge for the
/// first segment must leave the ordinary edge for the second.
#[test]
fn a_declared_peer_does_not_consume_the_propagated_peers_segment() {
    let lockfile = peer_context_lockfile(
        Some(("consumer@1.0.0", peer_declaring_metadata(["provider"]))),
        [(
            "consumer@1.0.0(provider@1.0.0)(provider@1.0.0)",
            snapshot_with_dependencies([
                ("provider", plain_dependency("1.0.0")),
                ("child-peer", alias_dependency("provider@1.0.0")),
            ]),
        )],
    );

    let versions = importer_locked_peer_versions(Some(&lockfile), "missing-importer");
    assert_eq!(
        versions,
        HashMap::from_iter([
            ("provider".to_string(), HashSet::from_iter(["1.0.0".to_string()])),
            ("child-peer".to_string(), HashSet::from_iter(["1.0.0".to_string()])),
        ]),
    );
}

/// Nothing ranks two ordinary aliases onto one provider, and guessing
/// between them would vary with map order.
#[test]
fn competing_ordinary_aliases_do_not_rename_the_segment() {
    let lockfile = peer_context_lockfile(
        Some(("consumer@1.0.0", peer_declaring_metadata([]))),
        [(
            "consumer@1.0.0(alias-provider@1.0.0)",
            snapshot_with_dependencies([
                ("one", alias_dependency("alias-provider@1.0.0")),
                ("other", alias_dependency("alias-provider@1.0.0")),
            ]),
        )],
    );

    let versions = importer_locked_peer_versions(Some(&lockfile), "missing-importer");
    assert_eq!(
        versions,
        HashMap::from_iter([(
            "alias-provider".to_string(),
            HashSet::from_iter(["1.0.0".to_string()]),
        )]),
    );
}

/// A `link:` edge resolves to a path, not a package version, so it can
/// never be the provider a suffix segment names.
#[test]
fn a_linked_dependency_is_not_a_peer_provider() {
    let lockfile = peer_context_lockfile(
        Some(("consumer@1.0.0", peer_declaring_metadata(["peer"]))),
        [(
            "consumer@1.0.0(alias-provider@1.0.0)",
            snapshot_with_dependencies([
                ("peer", alias_dependency("alias-provider@1.0.0")),
                ("local", link_dependency("../local")),
            ]),
        )],
    );

    let versions = importer_locked_peer_versions(Some(&lockfile), "missing-importer");
    assert_eq!(
        versions,
        HashMap::from_iter([("peer".to_string(), HashSet::from_iter(["1.0.0".to_string()]))]),
    );
}

/// A hashed suffix spells no peers out, so without the package metadata
/// naming them there is nothing left to recover them from.
#[test]
fn a_hashed_peer_suffix_without_package_metadata_records_nothing() {
    let lockfile = peer_context_lockfile(
        None,
        [(
            "consumer@1.0.0(0123456789abcdef0123456789abcdef)",
            snapshot_with_dependency("peer", plain_dependency("1.0.0")),
        )],
    );

    let versions = importer_locked_peer_versions(Some(&lockfile), "missing-importer");
    assert!(versions.is_empty());
}

/// A package that declares no peers still carries the peers its own
/// children resolved through it, so the suffix stays the only record of
/// them.
#[test]
fn explicit_peer_suffix_of_an_undeclared_peer_is_kept() {
    let lockfile = peer_context_lockfile(
        Some(("consumer@1.0.0", peer_declaring_metadata([]))),
        [(
            "consumer@1.0.0(peer@2.0.0)",
            snapshot_with_dependency("child", plain_dependency("1.0.0(peer@2.0.0)")),
        )],
    );

    let versions = importer_locked_peer_versions(Some(&lockfile), "missing-importer");
    assert_eq!(
        versions,
        HashMap::from_iter([("peer".to_string(), HashSet::from_iter(["2.0.0".to_string()]))]),
    );
}

/// A `packages:`/`snapshots:` pair keyed by the given depPaths, with
/// every field the peer-context tests do not read left empty.
fn peer_context_lockfile<const SNAPSHOTS: usize>(
    package: Option<(&str, pnpm_lockfile::PackageMetadata)>,
    snapshots: [(&str, pnpm_lockfile::SnapshotEntry); SNAPSHOTS],
) -> pnpm_lockfile::Lockfile {
    use pnpm_lockfile::{ComVer, Lockfile, LockfileVersion, PkgNameVerPeer};

    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).unwrap(),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: std::collections::HashMap::new(),
        packages: package.map(|(key, metadata)| {
            std::collections::HashMap::from([(PkgNameVerPeer::from_str(key).unwrap(), metadata)])
        }),
        snapshots: Some(
            snapshots
                .into_iter()
                .map(|(key, entry)| (PkgNameVerPeer::from_str(key).unwrap(), entry))
                .collect(),
        ),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

/// `packages:` metadata whose only content is the declared peer names,
/// each with a `*` range.
fn peer_declaring_metadata<const PEERS: usize>(
    peer_names: [&str; PEERS],
) -> pnpm_lockfile::PackageMetadata {
    use pnpm_lockfile::{DirectoryResolution, LockfileResolution, PackageMetadata};

    PackageMetadata {
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: "consumer".to_string(),
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: Some(
            peer_names.into_iter().map(|name| (name.to_string(), "*".to_string())).collect(),
        ),
        peer_dependencies_meta: None,
    }
}

fn snapshot_with_dependency(
    alias: &str,
    dependency: pnpm_lockfile::SnapshotDepRef,
) -> pnpm_lockfile::SnapshotEntry {
    snapshot_with_dependencies([(alias, dependency)])
}

fn snapshot_with_dependencies<const DEPS: usize>(
    dependencies: [(&str, pnpm_lockfile::SnapshotDepRef); DEPS],
) -> pnpm_lockfile::SnapshotEntry {
    use pnpm_lockfile::PkgName;

    SnapshotEntry {
        dependencies: Some(
            dependencies
                .into_iter()
                .map(|(alias, dependency)| (PkgName::parse(alias).unwrap(), dependency))
                .collect(),
        ),
        ..SnapshotEntry::default()
    }
}

fn plain_dependency(ver_peer: &str) -> pnpm_lockfile::SnapshotDepRef {
    pnpm_lockfile::SnapshotDepRef::Plain(pnpm_lockfile::PkgVerPeer::from_str(ver_peer).unwrap())
}

fn alias_dependency(key: &str) -> pnpm_lockfile::SnapshotDepRef {
    pnpm_lockfile::SnapshotDepRef::Alias(pnpm_lockfile::PkgNameVerPeer::from_str(key).unwrap())
}

fn link_dependency(target: &str) -> pnpm_lockfile::SnapshotDepRef {
    pnpm_lockfile::SnapshotDepRef::Link(target.to_string())
}

/// The locked peer names a lockfile yields for an importer it does not
/// list — the union `importer_locked_peer_context` folds over every
/// snapshot.
fn locked_peer_names(wanted_lockfile: Option<&pnpm_lockfile::Lockfile>) -> HashSet<String> {
    importer_locked_peer_versions(wanted_lockfile, "missing-importer").into_keys().collect()
}

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
    use pnpm_lockfile::{LockfileResolution, PkgName, PkgNameVer, TarballResolution};
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
            revision: None,
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
        dedupe_peer_dependents: true,
        all_preferred_versions: Arc::new(PreferredVersions::new()),
        override_bare_specifier: None,
        patched_dependencies: None,
        base_opts: ResolveOptions::default(),
        pick_lowest_direct: false,
        subdep_published_by: None,
        catalogs: pnpm_catalogs_types::Catalogs::new(),
        exclude_links_from_lockfile: false,
        lockfile_dir: None,
        modules_dir: None,
        peers_suffix_max_length: 1000,
        catalog_server: false,
        manifest_hook: None,
        overrides_hook: None,
        pnpmfile_hook: None,
    }
}

#[tokio::test]
async fn auto_installs_missing_required_peer() {
    let mut table = HashMap::default();
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
    let mut table = HashMap::default();
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
    let mut table = HashMap::default();
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
    let mut table = HashMap::default();
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
    let react_call_count = calls.iter().filter(|(name, _)| name == "react").count();
    assert_eq!(react_call_count, 1, "should not re-resolve react via a hoisted spec");

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"react"));
    assert!(direct.contains(&"react-dom"));
}

// ---------------------------------------------------------------------------
// `autoInstallPeers` test cases, each covering a single-importer scenario.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn auto_install_skips_optional_peers_without_preferred_versions() {
    let mut table = HashMap::default();
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
    let mut table = HashMap::default();
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
    let mut seeded = PreferredVersions::new();
    seeded.insert("peer-c".to_string(), peer_c_selectors);
    opts.all_preferred_versions = Arc::new(seeded);

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

#[tokio::test]
async fn auto_install_dedupes_via_range_intersection_when_identical() {
    let mut table = HashMap::default();
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
    let peer_c_entries: Vec<&DepPath> = result
        .peers_result
        .graph
        .keys()
        .filter(|dp| dp.to_string().starts_with("peer-c@"))
        .collect();
    assert_eq!(peer_c_entries.len(), 1, "expected one peer-c entry, got: {peer_c_entries:?}");
}

/// TS: `should return the intersection of two compatible ranges`
/// (`resolvePeers.ts:513`). Consumers wanting `2` and `^2.2.0` share
/// the intersected range `>=2.2.0 <3.0.0-0` — the resolver stub keys on
/// that exact specifier, so the single hoisted provider proves both the
/// intersection and its canonical rendering.
#[tokio::test]
async fn auto_installed_peer_uses_the_intersection_of_compatible_ranges() {
    let mut table = HashMap::default();
    table.insert(
        ("wants-peer-c-2".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-peer-c-2",
            "1.0.0",
            serde_json::json!({
                "name": "wants-peer-c-2",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "2" },
            }),
        ),
    );
    table.insert(
        ("wants-peer-c-2.2".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-peer-c-2.2",
            "1.0.0",
            serde_json::json!({
                "name": "wants-peer-c-2.2",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "^2.2.0" },
            }),
        ),
    );
    table.insert(
        ("peer-c".to_string(), ">=2.2.0 <3.0.0-0".to_string()),
        fake_result("peer-c", "2.2.5", serde_json::json!({ "name": "peer-c", "version": "2.2.5" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "wants-peer-c-2": "1.0.0",
        "wants-peer-c-2.2": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"peer-c"), "single intersected peer-c should be hoisted: {direct:?}");
    let peer_c_entries: Vec<&DepPath> = result
        .peers_result
        .graph
        .keys()
        .filter(|dp| dp.to_string().starts_with("peer-c@"))
        .collect();
    assert_eq!(peer_c_entries.len(), 1, "expected one peer-c entry, got: {peer_c_entries:?}");
    assert!(
        peer_c_entries[0].to_string().starts_with("peer-c@2.2.5"),
        "the provider must resolve through the intersected range: {peer_c_entries:?}",
    );
}

#[tokio::test]
async fn auto_install_does_not_install_when_no_intersection() {
    let mut table = HashMap::default();
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

#[test]
fn repeated_consumer_ranges_merge_into_the_unique_intersection() {
    let react = "^16.8 || ^17.0 || ^18.0 || ^19.0 || ^19.0.0-rc";
    let narrower = "^18.0.0 || ^19.0.0";
    let merged = ">=18.0.0 <19.0.0-0||>=19.0.0 <20.0.0-0";

    assert_eq!(merge_ranges(&[react, narrower], false).as_deref(), Some(merged));

    let mut repeated = vec![react; 10];
    repeated.push(narrower);

    assert_eq!(merge_ranges(&repeated, false).as_deref(), Some(merged));
}

/// Deduplication alone only sees ranges that are spelled identically.
/// Consumers reach the same peer through spellings that differ, and each
/// one that survives into the union multiplies the next intersection, so
/// the merged range has to stay bounded across them too.
#[test]
fn differently_spelled_equivalent_ranges_do_not_grow_the_merged_range() {
    let spellings = [
        "^16.8 || ^17.0 || ^18.0 || ^19.0 || ^19.0.0-rc",
        "^16.8.0 || ^17.0.0 || ^18.0.0 || ^19.0.0 || ^19.0.0-rc",
        ">=16.8 <17 || >=17 <18 || >=18 <19 || >=19 <20 || ^19.0.0-rc",
        "16.8 - 16.x || ^17.0 || ^18.0 || ^19.0 || ^19.0.0-rc",
    ];
    let merged =
        ">=16.8.0 <17.0.0-0||>=17.0.0 <18.0.0-0||>=18.0.0 <19.0.0-0||>=19.0.0-rc <20.0.0-0";

    assert_eq!(merge_ranges(&spellings, false).as_deref(), Some(merged));

    let repeated: Vec<&str> = spellings.iter().cycle().copied().take(200).collect();

    assert_eq!(merge_ranges(&repeated, false).as_deref(), Some(merged));
}

/// A scheme specifier is not a semver range, so intersecting it would
/// drop the peer instead of hoisting it.
#[test]
fn repeated_scheme_specifier_stays_verbatim() {
    assert_eq!(
        merge_ranges(&["workspace:^", "workspace:^"], false).as_deref(),
        Some("workspace:^"),
    );
}

/// The `@radix-ui/react-dialog` shape of pnpm/pnpm#13786: four paths
/// reach the same package, so every branch reports the identical missing
/// `react` peer again. One narrower declarer turns the merge into a real
/// intersection, and the stub resolves `react` only through the
/// deduplicated one — a merge that folds the duplicates back in reaches
/// a different range and leaves the peer unhoisted.
#[tokio::test]
async fn a_peer_reported_once_per_path_is_hoisted_through_one_intersection() {
    let react_range = "^16.8 || ^17.0 || ^18.0 || ^19.0 || ^19.0.0-rc";
    let mut table = HashMap::default();
    for (name, deps) in [
        ("dialog", vec!["dismissable-layer", "focus-scope", "portal", "primitive"]),
        ("dismissable-layer", vec!["primitive"]),
        ("focus-scope", vec!["primitive"]),
        ("portal", vec!["primitive"]),
        ("primitive", vec![]),
    ] {
        let dependencies: serde_json::Map<String, serde_json::Value> = deps
            .into_iter()
            .map(|dep| (dep.to_string(), serde_json::Value::from("1.0.0")))
            .collect();
        table.insert(
            (name.to_string(), "1.0.0".to_string()),
            fake_result(
                name,
                "1.0.0",
                serde_json::json!({
                    "name": name,
                    "version": "1.0.0",
                    "dependencies": dependencies,
                    "peerDependencies": { "react": react_range },
                }),
            ),
        );
    }
    table.insert(
        ("wants-react-18-or-19".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-react-18-or-19",
            "1.0.0",
            serde_json::json!({
                "name": "wants-react-18-or-19",
                "version": "1.0.0",
                "peerDependencies": { "react": "^18.0.0 || ^19.0.0" },
            }),
        ),
    );
    table.insert(
        ("react".to_string(), ">=18.0.0 <19.0.0-0||>=19.0.0 <20.0.0-0".to_string()),
        fake_result("react", "19.0.0", serde_json::json!({ "name": "react", "version": "19.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "dialog": "1.0.0",
        "wants-react-18-or-19": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"react"), "react should be hoisted: {direct:?}");
    let react_entries: Vec<&DepPath> = result
        .peers_result
        .graph
        .keys()
        .filter(|dep_path| dep_path.to_string().starts_with("react@"))
        .collect();
    assert_eq!(react_entries.len(), 1, "expected one react entry, got: {react_entries:?}");
    assert!(
        react_entries[0].to_string().starts_with("react@19.0.0"),
        "react must resolve through the intersected range: {react_entries:?}",
    );
}

#[tokio::test]
async fn auto_install_from_highest_match_installs_on_conflict() {
    let mut table = HashMap::default();
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

#[tokio::test]
async fn auto_install_reuses_peer_already_brought_by_a_sibling() {
    let mut table = HashMap::default();
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

#[tokio::test]
async fn auto_install_does_not_hoist_when_root_already_has_dep() {
    let mut table = HashMap::default();
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

/// An optional peer with a real `peerDependencies` entry whose
/// provider is resolved anywhere in the tree (here: deep under a
/// sibling) IS hoisted: every run-resolved version folds into the
/// preferred-versions set and the optional-peer hoist resolves the
/// peer against it after the wave (verified against pnpm 11.6.0 — the
/// `eslint` + `cosmiconfig-typescript-loader` shape, where `eslint`
/// gains `(jiti@x)`). For
/// <https://github.com/pnpm/pnpm/issues/12266>.
#[tokio::test]
async fn optional_peer_with_real_entry_is_hoisted_from_resolved_tree() {
    let mut table = HashMap::default();
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

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"opt"), "optional peer `opt` must be hoisted: {direct:?}");
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("needs-opt"),
        Some(&DepPath::from("needs-opt@1.0.0(opt@1.0.0)".to_string())),
    );
}

/// A peer declared only via `peerDependenciesMeta` on a direct package
/// (no `peerDependencies` entry — the `debug` / `supports-color`
/// shape) is an implied optional `"*"` peer and joins the optional-peer
/// hoist exactly like an explicitly declared optional peer: when a
/// satisfying version is already in the graph, it is deduped onto that
/// version and resolved for the requiring package. This keeps the
/// outcome identical whether the peer came from the registry manifest
/// or from a lockfile snapshot (which materializes implied peers into
/// `peerDependencies`), so resolution does not drift across lockfile
/// round-trips.
#[tokio::test]
async fn meta_only_optional_peer_is_hoisted_like_a_declared_optional_peer() {
    let mut table = HashMap::default();
    table.insert(
        ("needs-opt".to_string(), "1.0.0".to_string()),
        fake_result(
            "needs-opt",
            "1.0.0",
            serde_json::json!({
                "name": "needs-opt",
                "version": "1.0.0",
                "peerDependenciesMeta": { "opt": { "optional": true } },
            }),
        ),
    );
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

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"opt"), "meta-only peer `opt` must be hoisted: {direct:?}");
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("needs-opt"),
        Some(&DepPath::from("needs-opt@1.0.0(opt@1.0.0)".to_string())),
    );
}

#[tokio::test]
async fn real_peer_provider_from_direct_child_is_appended_as_hidden_direct_dep() {
    let mut table = HashMap::default();
    table.insert(
        ("host".to_string(), "1.0.0".to_string()),
        fake_result(
            "host",
            "1.0.0",
            serde_json::json!({
                "name": "host",
                "version": "1.0.0",
                "dependencies": {
                    "peer-user": "1.0.0",
                    "provider": "1.0.0",
                },
            }),
        ),
    );
    table.insert(
        ("peer-user".to_string(), "1.0.0".to_string()),
        fake_result(
            "peer-user",
            "1.0.0",
            serde_json::json!({
                "name": "peer-user",
                "version": "1.0.0",
                "peerDependencies": { "provider": "^1.0.0" },
            }),
        ),
    );
    table.insert(
        ("provider".to_string(), "1.0.0".to_string()),
        fake_result(
            "provider",
            "1.0.0",
            serde_json::json!({ "name": "provider", "version": "1.0.0" }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "host": "1.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("provider"),
        Some(&DepPath::from("provider@1.0.0".to_string())),
        "provider should be available to the importer peer pass without being a manifest dep",
    );
    assert!(
        result
            .peers_result
            .graph
            .contains_key(&DepPath::from("peer-user@1.0.0(provider@1.0.0)".to_string())),
        "peer-user should resolve provider from host's child dependency",
    );
}

#[tokio::test]
async fn meta_only_peer_provider_from_direct_child_is_appended_as_hidden_direct_dep() {
    let mut table = HashMap::default();
    table.insert(
        ("host".to_string(), "1.0.0".to_string()),
        fake_result(
            "host",
            "1.0.0",
            serde_json::json!({
                "name": "host",
                "version": "1.0.0",
                "dependencies": {
                    "peer-user": "1.0.0",
                    "provider": "1.0.0",
                },
            }),
        ),
    );
    table.insert(
        ("peer-user".to_string(), "1.0.0".to_string()),
        fake_result(
            "peer-user",
            "1.0.0",
            serde_json::json!({
                "name": "peer-user",
                "version": "1.0.0",
                "peerDependenciesMeta": { "provider": { "optional": true } },
            }),
        ),
    );
    table.insert(
        ("provider".to_string(), "1.0.0".to_string()),
        fake_result(
            "provider",
            "1.0.0",
            serde_json::json!({ "name": "provider", "version": "1.0.0" }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "host": "1.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("provider"),
        Some(&DepPath::from("provider@1.0.0".to_string())),
        "a resolved meta-only peer feeds auto-installed hidden direct deps like a declared one",
    );
    assert!(
        result
            .peers_result
            .graph
            .contains_key(&DepPath::from("peer-user@1.0.0(provider@1.0.0)".to_string())),
        "meta-only peers resolve in the final peer graph when provider is in scope",
    );
}

#[tokio::test]
async fn auto_install_does_not_install_same_missing_peer_twice() {
    let mut table = HashMap::default();
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

/// Prefer the peer dependency version already used in the root: when
/// the importer declares the peer itself, its pinned version wins via
/// the importer-peerDependencies seed — even if `latest` would resolve
/// higher.
#[tokio::test]
async fn auto_install_prefers_peer_version_pinned_in_importer_peerdeps() {
    let mut table = HashMap::default();
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

#[tokio::test]
async fn auto_install_hoisted_peer_dep_reuses_regular_dep_version() {
    let mut table = HashMap::default();
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
/// The dereference is importer-only.
#[tokio::test]
async fn catalog_protocol_on_direct_dep_is_rewritten() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result("foo", "1.2.0", serde_json::json!({ "name": "foo", "version": "1.2.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "catalog:" }));

    let mut catalogs = pnpm_catalogs_types::Catalogs::new();
    catalogs.insert(
        "default".to_string(),
        std::iter::once(("foo".to_string(), "^1.0.0".to_string())).collect(),
    );

    let opts = ResolveImporterOptions { catalogs, ..default_opts() };
    let result =
        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();
    assert_eq!(result.resolved_tree.direct.len(), 1);
    assert_eq!(result.resolved_tree.direct[0].alias, "foo");
    let calls = resolver.calls.lock().unwrap();
    assert_eq!(&*calls, &[("foo".to_string(), "^1.0.0".to_string())]);
}

/// A misconfigured `catalog:` entry (here: missing alias) short-
/// circuits resolution with the `CATALOG_ENTRY_NOT_FOUND_FOR_SPEC`
/// error rather than falling through to `SpecNotSupported`.
#[tokio::test]
async fn catalog_misconfiguration_surfaces_pnpm_error_code() {
    let resolver = StubResolver { table: HashMap::default(), calls: Mutex::new(Vec::new()) };
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
        other => panic!("expected CatalogMisconfiguration, got {other:?}"),
    }
}

/// Build a [`ResolveResult`] for an `npm:`-aliased install. `local_alias`
/// is the alias the importer uses in `node_modules/` (and in
/// `parentPkgs`); `real_name`/`version` identify the resolved package.
/// Mirrors the real npm-resolver's behaviour: the result carries the
/// local alias, while `name_ver` and `id` point at the underlying
/// package.
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
/// Pacquet's `resolve_peers` walks synchronously with an `in_progress`
/// set, so the deadlock that hit pnpm does not occur here — but the
/// scenario has to keep terminating with a graph entry for the aliased
/// root and for each pair of mutually-peer-depending leaves.
///
/// Layout (from the bug report): an aliased install `a@npm:a-real`
/// pulls in `b-real` and `c-real`. Each of those depends on one half
/// of a mutual-peer pair (`x` ↔ `y`) and peer-depends on the aliased
/// root (`a@npm:a-real`). The hoist loop auto-installs `x` and `y` at
/// the importer level, where their cycle surfaces.
#[tokio::test]
async fn aliased_install_with_transitive_mutual_peer_cycle_terminates() {
    let mut table = HashMap::default();
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

    let dep_paths: HashSet<String> =
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
    use pnpm_package_manifest::DependencyGroup;
    use pnpm_resolving_resolver_base::{
        ResolveFuture, ResolveOptions, ResolveResult, Resolver, WantedDependency,
    };
    use pretty_assertions::assert_eq;
    use rustc_hash::FxHashMap as HashMap;
    use std::sync::Mutex;

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
                seen: Mutex::new(HashMap::default()),
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
            query: &'a pnpm_resolving_resolver_base::LatestQuery,
            opts: &'a ResolveOptions,
        ) -> pnpm_resolving_resolver_base::ResolveLatestFuture<'a> {
            self.inner.resolve_latest(query, opts)
        }
    }

    fn one_dep_one_subdep_table() -> HashMap<(String, String), ResolveResult> {
        let mut table = HashMap::default();
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

/// `hoistPeers` is `autoInstallPeers || dedupePeerDependents`: with both
/// off, a missing optional peer stays missing even though a preferred
/// version is in scope, so the packages that declare it keep an
/// unsuffixed snapshot.
#[tokio::test]
async fn both_hoist_settings_off_leaves_the_optional_peer_missing() {
    let mut table = HashMap::default();
    table.insert(
        ("abc".to_string(), "1.0.0".to_string()),
        fake_result(
            "abc",
            "1.0.0",
            serde_json::json!({
                "name": "abc",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "^1.0.0" },
                "peerDependenciesMeta": { "peer-c": { "optional": true } },
            }),
        ),
    );
    table.insert(
        ("peer-c".to_string(), "1.0.0".to_string()),
        fake_result("peer-c", "1.0.0", serde_json::json!({ "name": "peer-c", "version": "1.0.0" })),
    );
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "abc": "1.0.0" }));

    // A sibling importer already resolved the peer, so the optional
    // hoist would have a version to pick.
    let seeded_preferred_versions = || {
        let mut selectors = VersionSelectors::new();
        selectors
            .insert("1.0.0".to_string(), VersionSelectorEntry::Plain(VersionSelectorType::Version));
        PreferredVersions::from([("peer-c".to_string(), selectors)])
    };

    let hoisting_off = ResolveImporterOptions {
        auto_install_peers: false,
        dedupe_peer_dependents: false,
        all_preferred_versions: Arc::new(seeded_preferred_versions()),
        ..default_opts()
    };
    let resolver = StubResolver { table: table.clone(), calls: Mutex::new(Vec::new()) };
    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], hoisting_off)
        .await
        .unwrap();
    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert_eq!(direct, ["abc"]);
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("abc"),
        Some(&DepPath::from("abc@1.0.0".to_string())),
    );

    // `dedupePeerDependents` alone still hoists it, without
    // `autoInstallPeers`.
    let dedupe_only = ResolveImporterOptions {
        auto_install_peers: false,
        dedupe_peer_dependents: true,
        all_preferred_versions: Arc::new(seeded_preferred_versions()),
        ..default_opts()
    };
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let result =
        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], dedupe_only).await.unwrap();
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("abc"),
        Some(&DepPath::from("abc@1.0.0(peer-c@1.0.0)".to_string())),
    );
}
