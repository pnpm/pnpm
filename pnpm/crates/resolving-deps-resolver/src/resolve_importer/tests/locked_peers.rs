use super::{
    Arc, DepPath, DependencyGroup, EXISTING_VERSION_SELECTOR_WEIGHT, HashMap, HashSet, Mutex,
    PreferredVersions, SnapshotEntry, StubResolver, VersionSelectorEntry, VersionSelectorType,
    VersionSelectorWithWeight, VersionSelectors, alias_dependency, assert_eq, default_opts,
    fake_manifest, fake_result, importer_locked_peer_versions, link_dependency, locked_peer_names,
    peer_context_lockfile, peer_declaring_metadata, plain_dependency, resolve_importer,
    snapshot_with_dependencies, snapshot_with_dependency,
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
