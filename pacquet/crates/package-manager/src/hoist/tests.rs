//! Unit tests for the hoist algorithm.
//!
//! These exercise [`super::get_hoisted_dependencies`] in isolation
//! against synthetic graphs — the full install integration tests live
//! in `crates/package-manager/tests/` and `crates/cli/tests/`.

use super::{
    DirectDepsByImporter, HoistGraphNode, HoistInputs, HoistedDependencies,
    build_direct_deps_by_importer, build_hoist_graph, get_hoisted_dependencies,
};
use pacquet_config::matcher::create_matcher;
use pacquet_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgName, PkgVerPeer, ProjectSnapshot,
    RegistryResolution, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef,
    SnapshotEntry,
};
use pacquet_modules_yaml::HoistKind;
use pacquet_package_manifest::DependencyGroup;
use pretty_assertions::assert_eq;
use ssri::Integrity;
use std::collections::{HashMap, HashSet};

fn name(text: &str) -> PkgName {
    PkgName::parse(text).expect("parse pkg name")
}

fn ver(text: &str) -> PkgVerPeer {
    text.parse().expect("parse PkgVerPeer")
}

fn key(name_text: &str, version: &str) -> PackageKey {
    PackageKey::new(name(name_text), ver(version))
}

fn integrity() -> Integrity {
    "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        .parse()
        .expect("parse integrity")
}

fn metadata(has_bin: bool) -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution { integrity: integrity() }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: Some(has_bin),
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn pats<const LEN: usize>(patterns: [&str; LEN]) -> Vec<String> {
    patterns.iter().map(std::string::ToString::to_string).collect()
}

/// `(alias, dep_name, dep_version)` triple describing one entry in
/// a snapshot's dependency map. `alias == dep_name` for plain deps;
/// `alias != dep_name` denotes an npm-alias.
type LockfileDataDep<'a> = (&'a str, &'a str, &'a str);

/// `(name, version, dependencies, has_bin)` row describing one
/// snapshot entry to be assembled by [`make_lockfile_data`].
type LockfileDataRow<'a> = (&'a str, &'a str, &'a [LockfileDataDep<'a>], bool);

/// Helper: build (snapshots, packages) from a flat list of
/// `(name, ver, deps_by_alias_to_(name,ver), has_bin)` tuples.
fn make_lockfile_data(
    rows: &[LockfileDataRow<'_>],
) -> (HashMap<PackageKey, SnapshotEntry>, HashMap<PackageKey, PackageMetadata>) {
    let mut snapshots: HashMap<PackageKey, SnapshotEntry> = HashMap::new();
    let mut packages: HashMap<PackageKey, PackageMetadata> = HashMap::new();
    for (n, v, deps, has_bin) in rows {
        let pkg_key = key(n, v);
        let mut dep_map: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
        for (alias, dep_name, dep_ver) in *deps {
            let dep_alias = name(alias);
            let dep_ref = if alias == dep_name {
                SnapshotDepRef::Plain(ver(dep_ver))
            } else {
                // npm-alias: alias != target name
                SnapshotDepRef::Alias(PackageKey::new(name(dep_name), ver(dep_ver)))
            };
            dep_map.insert(dep_alias, dep_ref);
        }
        let snapshot = SnapshotEntry {
            dependencies: if dep_map.is_empty() { None } else { Some(dep_map) },
            ..Default::default()
        };
        snapshots.insert(pkg_key.clone(), snapshot);
        packages.insert(pkg_key, metadata(*has_bin));
    }
    (snapshots, packages)
}

fn root_direct_deps(pairs: &[(&str, &str, &str)]) -> DirectDepsByImporter {
    let mut deps: HashMap<String, PackageKey> = HashMap::new();
    for (alias, n, v) in pairs {
        deps.insert(alias.to_string(), key(n, v));
    }
    HashMap::from([(".".to_string(), deps)])
}

/// Sanity: empty graph short-circuits. Mirrors upstream's
/// `if (Object.keys(opts.graph ?? {}).length === 0) return null`.
#[test]
fn empty_graph_returns_none() {
    let graph: HashMap<PackageKey, HoistGraphNode> = HashMap::new();
    let direct: DirectDepsByImporter = HashMap::new();
    let skipped: HashSet<PackageKey> = HashSet::new();
    let input = HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct,
        skipped: &skipped,
        private_pattern: create_matcher(&pats(["*"])),
        public_pattern: create_matcher(&[]),
    };
    assert!(get_hoisted_dependencies(&input).is_none());
}

/// Default `hoistPattern: ["*"]` hoists every transitive (privately).
/// Direct deps don't get hoisted because they're already at the root.
#[test]
fn star_pattern_hoists_all_transitives_privately() {
    // root → a; a → b
    let (snapshots, packages) = make_lockfile_data(&[
        ("a", "1.0.0", &[("b", "b", "1.0.0")], false),
        ("b", "1.0.0", &[], false),
    ]);
    let graph = build_hoist_graph(&snapshots, &packages);
    let direct = root_direct_deps(&[("a", "a", "1.0.0")]);
    let skipped = HashSet::new();
    let result = get_hoisted_dependencies(&HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct,
        skipped: &skipped,
        private_pattern: create_matcher(&pats(["*"])),
        public_pattern: create_matcher(&[]),
    })
    .expect("non-empty graph");

    // Direct dep `a` is NOT in hoisted (already at the root).
    // Transitive `b` IS, with kind=private.
    assert_eq!(
        kinds_for(&result.hoisted_dependencies, "b@1.0.0"),
        vec![("b".to_string(), HoistKind::Private)],
    );
    assert!(
        !result.hoisted_dependencies.contains_key("a@1.0.0"),
        "direct deps must not be hoisted: {:?}",
        result.hoisted_dependencies.get("a@1.0.0"),
    );
}

/// `publicHoistPattern: ["*"]` hoists every transitive publicly. Once
/// a transitive is hoisted publicly, its sibling at a deeper level
/// shouldn't claim the same alias again.
#[test]
fn star_public_pattern_hoists_all_publicly() {
    let (snapshots, packages) = make_lockfile_data(&[
        ("a", "1.0.0", &[("b", "b", "1.0.0")], false),
        ("b", "1.0.0", &[], false),
    ]);
    let graph = build_hoist_graph(&snapshots, &packages);
    let direct = root_direct_deps(&[("a", "a", "1.0.0")]);
    let skipped = HashSet::new();
    let result = get_hoisted_dependencies(&HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct,
        skipped: &skipped,
        private_pattern: create_matcher(&[]),
        public_pattern: create_matcher(&pats(["*"])),
    })
    .expect("non-empty graph");

    assert_eq!(
        kinds_for(&result.hoisted_dependencies, "b@1.0.0"),
        vec![("b".to_string(), HoistKind::Public)],
    );
}

/// Public pattern wins ties — when both private and public patterns
/// match the same alias, the alias goes public. Mirrors upstream's
/// `if (publicMatcher(alias)) return 'public'; if (privateMatcher(alias)) return 'private'`.
#[test]
fn public_pattern_wins_ties() {
    let (snapshots, packages) = make_lockfile_data(&[
        ("eslint-x", "1.0.0", &[("eslint-y", "eslint-y", "1.0.0")], false),
        ("eslint-y", "1.0.0", &[], false),
    ]);
    let graph = build_hoist_graph(&snapshots, &packages);
    let direct = root_direct_deps(&[("eslint-x", "eslint-x", "1.0.0")]);
    let skipped = HashSet::new();
    let result = get_hoisted_dependencies(&HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct,
        skipped: &skipped,
        private_pattern: create_matcher(&pats(["*"])),
        public_pattern: create_matcher(&pats(["*eslint*"])),
    })
    .expect("non-empty graph");

    // eslint-y matches both `*` and `*eslint*` — public wins.
    assert_eq!(
        kinds_for(&result.hoisted_dependencies, "eslint-y@1.0.0"),
        vec![("eslint-y".to_string(), HoistKind::Public)],
    );
}

/// Negation in `hoistPattern` — `["*", "!banned"]` hoists everything
/// except aliases named `banned`.
#[test]
fn negation_pattern_excludes_alias() {
    let (snapshots, packages) = make_lockfile_data(&[
        ("a", "1.0.0", &[("banned", "banned", "1.0.0"), ("ok", "ok", "1.0.0")], false),
        ("banned", "1.0.0", &[], false),
        ("ok", "1.0.0", &[], false),
    ]);
    let graph = build_hoist_graph(&snapshots, &packages);
    let direct = root_direct_deps(&[("a", "a", "1.0.0")]);
    let skipped = HashSet::new();
    let result = get_hoisted_dependencies(&HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct,
        skipped: &skipped,
        private_pattern: create_matcher(&pats(["*", "!banned"])),
        public_pattern: create_matcher(&[]),
    })
    .expect("non-empty graph");

    assert!(result.hoisted_dependencies.contains_key("ok@1.0.0"));
    assert!(
        !result.hoisted_dependencies.contains_key("banned@1.0.0"),
        "banned must be excluded by `!banned` ignore pattern",
    );
}

/// First-seen-wins per alias. With `a -> shared@1; b -> shared@2`,
/// both at depth 1, the lex-first sorted entry decides which one
/// becomes the hoisted version. Pacquet's sort is by `(depth, key)`
/// — the depth-1 walks alphabetically by parent (`a` before `b`),
/// so `shared@1` wins.
#[test]
fn first_seen_wins_per_alias() {
    let (snapshots, packages) = make_lockfile_data(&[
        ("a", "1.0.0", &[("shared", "shared", "1.0.0")], false),
        ("b", "1.0.0", &[("shared", "shared", "2.0.0")], false),
        ("shared", "1.0.0", &[], false),
        ("shared", "2.0.0", &[], false),
    ]);
    let graph = build_hoist_graph(&snapshots, &packages);
    let direct = root_direct_deps(&[("a", "a", "1.0.0"), ("b", "b", "1.0.0")]);
    let skipped = HashSet::new();
    let result = get_hoisted_dependencies(&HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct,
        skipped: &skipped,
        private_pattern: create_matcher(&pats(["*"])),
        public_pattern: create_matcher(&[]),
    })
    .expect("non-empty graph");

    // `shared@1.0.0` (under `a`) wins because `a` sorts before `b`.
    // `shared@2.0.0` (under `b`) is NOT hoisted.
    assert!(result.hoisted_dependencies.contains_key("shared@1.0.0"));
    assert!(
        !result.hoisted_dependencies.contains_key("shared@2.0.0"),
        "second-seen alias must not be hoisted",
    );
}

/// Direct-dep aliases of the root importer seed `hoistedAliases`,
/// blocking same-named transitives from being hoisted under different
/// versions. Mirrors upstream's `currentSpecifiers` parameter.
#[test]
fn direct_dep_blocks_same_alias_transitive() {
    // root → has-shared@1; has-shared@1 → shared@2 (transitive).
    // Also direct-dep `shared@1` — should block `shared@2` from being hoisted.
    let (snapshots, packages) = make_lockfile_data(&[
        ("has-shared", "1.0.0", &[("shared", "shared", "2.0.0")], false),
        ("shared", "1.0.0", &[], false),
        ("shared", "2.0.0", &[], false),
    ]);
    let graph = build_hoist_graph(&snapshots, &packages);
    let direct =
        root_direct_deps(&[("has-shared", "has-shared", "1.0.0"), ("shared", "shared", "1.0.0")]);
    let skipped = HashSet::new();
    let result = get_hoisted_dependencies(&HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct,
        skipped: &skipped,
        private_pattern: create_matcher(&pats(["*"])),
        public_pattern: create_matcher(&[]),
    })
    .expect("non-empty graph");

    // `shared` is a direct dep — its alias is in `currentSpecifiers`.
    // The transitive `shared@2.0.0` must NOT be hoisted under that
    // alias, because the root already owns it at v1.
    assert!(
        !result.hoisted_dependencies.contains_key("shared@2.0.0"),
        "direct-dep `shared@1` must block hoisting of transitive `shared@2`",
    );
}

/// Skipped snapshots are excluded from `hoistedDependencies` even when
/// the matcher accepts them. Mirrors upstream's
/// `if (node?.depPath == null || opts.skipped.has(node.depPath)) continue`.
#[test]
fn skipped_snapshot_is_excluded() {
    let (snapshots, packages) = make_lockfile_data(&[
        ("a", "1.0.0", &[("opt", "opt", "1.0.0")], false),
        ("opt", "1.0.0", &[], false),
    ]);
    let graph = build_hoist_graph(&snapshots, &packages);
    let direct = root_direct_deps(&[("a", "a", "1.0.0")]);
    let mut skipped = HashSet::new();
    skipped.insert(key("opt", "1.0.0"));
    let result = get_hoisted_dependencies(&HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct,
        skipped: &skipped,
        private_pattern: create_matcher(&pats(["*"])),
        public_pattern: create_matcher(&[]),
    })
    .expect("non-empty graph");

    assert!(
        !result.hoisted_dependencies.contains_key("opt@1.0.0"),
        "skipped snapshot must not appear in hoistedDependencies",
    );
    // ...but the symlink-by-node-id map DOES carry the entry —
    // upstream records it before the skipped check (see
    // [`hoistGraph`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/hoist/src/index.ts#L207-L267)),
    // and pacquet preserves that ordering for parity. The symlink
    // pass [`super::symlink_hoisted_dependencies`] is what actually
    // filters skipped node IDs out of the symlink work (the
    // `graph.get(node_id)` guard alone isn't enough — `graph`
    // contains every snapshot in `snapshots`, skipped or not, since
    // `build_hoist_graph` only filters by missing metadata). The
    // entry rides along in the map for any consumer that wants to
    // inspect what was considered, and the explicit skip check at
    // the symlink site prevents a dangling slot symlink.
    assert!(result.hoisted_dependencies_by_node_id.contains_key(&key("opt", "1.0.0")));
}

/// `symlink_hoisted_dependencies` filters entries whose key is in
/// the skip set. Regression for PR [#485] Copilot review: without the
/// filter, a prod dependency with an optional transitive child
/// would still get a dangling hoist symlink to the child's
/// virtual-store slot, which `CreateVirtualStore` skipped.
///
/// [#485]: https://github.com/pnpm/pacquet/pull/485
#[test]
fn symlink_skips_dropped_nodes() {
    use crate::VirtualStoreLayout;
    use pacquet_lockfile::PkgName;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let virtual_store_dir = dir.path().join("node_modules/.pacquet");
    let private_hoisted = virtual_store_dir.join("node_modules");
    let public_hoisted = dir.path().join("node_modules");
    std::fs::create_dir_all(&virtual_store_dir).unwrap();

    // Two-node hoist map: `kept@1.0.0` survives, `dropped@1.0.0` is
    // in the skip set. The "graph" entries exist for both (mirroring
    // what `build_hoist_graph` produces in production — it doesn't
    // filter by skip).
    let kept_key = key("kept", "1.0.0");
    let dropped_key = key("dropped", "1.0.0");
    let mut hoisted: HashMap<PackageKey, HashMap<String, HoistKind>> = HashMap::new();
    hoisted.insert(kept_key.clone(), HashMap::from([("kept".to_string(), HoistKind::Private)]));
    hoisted
        .insert(dropped_key.clone(), HashMap::from([("dropped".to_string(), HoistKind::Private)]));

    let mut graph: HashMap<PackageKey, HoistGraphNode> = HashMap::new();
    graph.insert(
        kept_key.clone(),
        HoistGraphNode {
            name: PkgName::parse("kept").unwrap(),
            children: HashMap::new(),
            has_bin: false,
        },
    );
    graph.insert(
        dropped_key.clone(),
        HoistGraphNode {
            name: PkgName::parse("dropped").unwrap(),
            children: HashMap::new(),
            has_bin: false,
        },
    );

    // Pre-create just the kept snapshot's slot so its symlink has a
    // valid target. The dropped snapshot's slot is intentionally
    // absent — without the skip filter, the symlink pass would try
    // to create a link pointing at it.
    let layout = VirtualStoreLayout::legacy(
        &virtual_store_dir,
        pacquet_config::default_virtual_store_dir_max_length() as usize,
    );
    std::fs::create_dir_all(layout.slot_dir(&kept_key).join("node_modules/kept")).unwrap();

    let mut skipped: HashSet<PackageKey> = HashSet::new();
    skipped.insert(dropped_key);

    super::symlink_hoisted_dependencies(
        &hoisted,
        &graph,
        &layout,
        &private_hoisted,
        &public_hoisted,
        &skipped,
    )
    .expect("symlink pass must succeed");

    // Use `symlink_metadata` rather than `exists` because a
    // dangling symlink (the regression) makes `exists()` return
    // false anyway — `exists()` follows the link. Need to detect
    // the symlink itself, not its target.
    assert!(
        std::fs::symlink_metadata(private_hoisted.join("kept")).is_ok(),
        "kept alias must be hoisted (symlink present)",
    );
    assert!(
        std::fs::symlink_metadata(private_hoisted.join("dropped")).is_err(),
        "dropped (skipped) alias must NOT have a hoist symlink — would be dangling",
    );
}

/// Bins of privately-hoisted aliases land in `hoisted_aliases_with_bins`.
/// Bins of publicly-hoisted aliases do NOT (they share `<root>/node_modules/.bin`
/// with direct deps and are linked by the regular direct-deps pass).
#[test]
fn private_hoist_with_bins_collected_for_bin_link() {
    let (snapshots, packages) = make_lockfile_data(&[
        ("a", "1.0.0", &[("with-bin", "with-bin", "1.0.0"), ("no-bin", "no-bin", "1.0.0")], false),
        ("with-bin", "1.0.0", &[], true),
        ("no-bin", "1.0.0", &[], false),
    ]);
    let graph = build_hoist_graph(&snapshots, &packages);
    let direct = root_direct_deps(&[("a", "a", "1.0.0")]);
    let skipped = HashSet::new();
    let result = get_hoisted_dependencies(&HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct,
        skipped: &skipped,
        private_pattern: create_matcher(&pats(["*"])),
        public_pattern: create_matcher(&[]),
    })
    .expect("non-empty graph");

    assert!(result.hoisted_aliases_with_bins.contains(&"with-bin".to_string()));
    assert!(!result.hoisted_aliases_with_bins.contains(&"no-bin".to_string()));
}

/// Public hoist with bin: alias does NOT contribute to
/// `hoisted_aliases_with_bins` (only private-side hoists do, since
/// public-side bins are linked by the direct-deps pass).
#[test]
fn public_hoist_does_not_contribute_to_bin_aliases() {
    let (snapshots, packages) = make_lockfile_data(&[
        ("a", "1.0.0", &[("eslint-bin", "eslint-bin", "1.0.0")], false),
        ("eslint-bin", "1.0.0", &[], true),
    ]);
    let graph = build_hoist_graph(&snapshots, &packages);
    let direct = root_direct_deps(&[("a", "a", "1.0.0")]);
    let skipped = HashSet::new();
    let result = get_hoisted_dependencies(&HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct,
        skipped: &skipped,
        private_pattern: create_matcher(&[]),
        public_pattern: create_matcher(&pats(["*eslint*"])),
    })
    .expect("non-empty graph");

    assert!(result.hoisted_aliases_with_bins.is_empty());
}

/// `build_direct_deps_by_importer` reads from `importers["."].dependencies`
/// (and dev/optional per the requested groups) and produces the
/// alias → key map the hoist pass expects.
#[test]
fn build_direct_deps_by_importer_collects_from_importers() {
    let mut importers: HashMap<String, ProjectSnapshot> = HashMap::new();
    let mut deps: ResolvedDependencyMap = HashMap::new();
    deps.insert(
        name("a"),
        ResolvedDependencySpec { specifier: "^1".to_string(), version: ver("1.0.0").into() },
    );
    importers.insert(
        ".".to_string(),
        ProjectSnapshot { dependencies: Some(deps), ..Default::default() },
    );

    let result = build_direct_deps_by_importer(
        &importers,
        [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    );
    let dot = result.get(".").expect("root importer present");
    assert_eq!(dot.get("a"), Some(&key("a", "1.0.0")));
}

/// Round-trip: `build_hoist_graph` on a tiny snapshot set produces the
/// expected children map.
#[test]
fn build_hoist_graph_walks_dependencies() {
    let (snapshots, packages) = make_lockfile_data(&[
        ("a", "1.0.0", &[("b", "b", "1.0.0")], true),
        ("b", "1.0.0", &[], false),
    ]);
    let graph = build_hoist_graph(&snapshots, &packages);
    let a_node = graph.get(&key("a", "1.0.0")).expect("a node");
    assert_eq!(a_node.children.get("b"), Some(&key("b", "1.0.0")));
    assert!(a_node.has_bin);

    let b_node = graph.get(&key("b", "1.0.0")).expect("b node");
    assert!(b_node.children.is_empty());
    assert!(!b_node.has_bin);
}

/// Helper: extract the (alias, kind) pairs at a given snapshot key
/// for assertion purposes. Sorted for stable comparison.
fn kinds_for(map: &HoistedDependencies, key: &str) -> Vec<(String, HoistKind)> {
    let mut pairs: Vec<_> = map
        .get(key)
        .map(|inner| inner.iter().map(|(pkg_key, v)| (pkg_key.clone(), *v)).collect())
        .unwrap_or_default();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}
