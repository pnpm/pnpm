use super::super::{
    BTreeMap, Config, HashMap, HashSet, PackageKey, PackageMetadata, Path, PathBuf, Prefix,
    SkippedSnapshots, SnapshotEntry, build_direct_deps_by_importer, create_matcher,
    get_hoisted_dependencies,
};

/// Pre-computed hoist plan threaded across the install pipeline so
/// the dedupe pass in [`crate::SymlinkDirectDependencies`] (which
/// runs before the on-disk hoist phase in pacquet's ordering) can
/// fold publicly-hoisted aliases into root's target map. The on-disk
/// hoist phase later consumes the same [`crate::HoistResult`] instead of
/// re-running the traversal.
pub struct HoistPlan {
    pub graph: HashMap<PackageKey, crate::HoistGraphNode>,
    pub result: crate::HoistResult,
    pub skipped: HashSet<PackageKey>,
}
/// Compute the in-memory hoist plan. Returns `None` when nothing
/// should be hoisted today (no patterns, no lockfile graph, or the
/// install is going through the hoisted linker). Side-effect-free:
/// the on-disk symlinks happen later in the pipeline. Same input
/// gating as the legacy in-place block in [`crate::install_frozen_lockfile::InstallFrozenLockfile::run`].
/// `hoist-workspace-packages` input: every named non-root project's
/// `name → absolute project dir`, the shape v11 builds from
/// `allProjects` for its `hoistedWorkspacePackages` map. The root
/// project itself is excluded — its dir *is* where the hoisted
/// modules live.
#[must_use]
pub fn workspace_packages_for_hoist(
    workspace_root: &Path,
    project_manifests: &[(PathBuf, &pnpm_package_manifest::PackageManifest)],
) -> indexmap::IndexMap<String, PathBuf> {
    project_manifests
        .iter()
        .filter(|(project_dir, _)| project_dir != workspace_root)
        .filter_map(|(project_dir, manifest)| {
            let name = manifest.value().get("name")?.as_str()?;
            Some((name.to_string(), project_dir.clone()))
        })
        .collect()
}
#[expect(
    clippy::too_many_arguments,
    reason = "bundles every lockfile/config axis one hoist plan needs; both call sites pass the same shapes"
)]
pub fn compute_hoist_plan(
    config: &Config,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    importers: &HashMap<String, pnpm_lockfile::ProjectSnapshot>,
    dependency_groups: &[pnpm_package_manifest::DependencyGroup],
    skipped: &SkippedSnapshots,
    is_hoisted: bool,
    hoisted_workspace_packages: Option<&indexmap::IndexMap<String, PathBuf>>,
) -> Option<HoistPlan> {
    if is_hoisted {
        return None;
    }
    // Independent of the empty patterns
    // [`Config::apply_virtual_store_only_derivation`] leaves behind, so a
    // caller that sets the flag without going through `Config::current`
    // still gets no hoisting.
    if config.virtual_store_only {
        return None;
    }
    if config.hoist_pattern.is_none() && config.public_hoist_pattern.is_none() {
        return None;
    }
    let (Some(snaps), Some(pkgs)) = (snapshots, packages) else { return None };
    let private_pattern = create_matcher(config.hoist_pattern.as_deref().unwrap_or(&[]));
    let public_pattern = create_matcher(config.public_hoist_pattern.as_deref().unwrap_or(&[]));
    // Static fast-path: when both compiled matchers come from empty
    // pattern lists (`Some([])`), there's no alias they could match,
    // so the traversal would visit every node only to drop every child.
    // Skip the graph-build + walk entirely.
    if private_pattern.is_empty() && public_pattern.is_empty() {
        return None;
    }
    let graph = crate::build_hoist_graph_with_max_length(
        snaps,
        pkgs,
        config.virtual_store_dir_max_length as usize,
    );
    // Walk every importer's direct deps so transitives unique to a
    // workspace project still get privately hoisted into the shared
    // `<vs>/node_modules` and contribute to `hoistedDependencies`.
    // The `link:` workspace-sibling entries `build_direct_deps_by_importer`
    // sees are skipped via [`pnpm_lockfile::ImporterDepVersion::as_regular`].
    let direct_deps = build_direct_deps_by_importer(importers, dependency_groups.iter().copied());
    // `HoistInputs` takes `&HashSet<PackageKey>`; build it once from
    // the outer `SkippedSnapshots` by cloning the small skip set
    // (typically 0-100 entries). Stored on [`HoistPlan`] so the
    // later on-disk pass can reuse the exact same set the traversal saw.
    let hoist_skipped: HashSet<PackageKey> = skipped.iter().cloned().collect();
    let result = get_hoisted_dependencies(&crate::HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct_deps,
        skipped: &hoist_skipped,
        private_pattern,
        public_pattern,
        hoisted_workspace_packages,
    })?;
    Some(HoistPlan { graph, result, skipped: hoist_skipped })
}
/// Build the `<alias → resolved-target-dir>` map for every publicly-
/// hoisted entry that will land in root's `node_modules/`. Pacquet
/// runs the dedupe pass before the on-disk hoist phase, so this map
/// lets the dedupe see the aliases it would otherwise miss — by the
/// time the linker reads `<root>/node_modules/`, the public-hoist
/// symlinks are already there because hoist ran first.
///
/// Skipped snapshots are dropped (their slot dir doesn't exist on
/// disk), missing-in-graph entries are dropped, and only `Public`
/// hoists contribute (private hoists land in the virtual store's
/// own `node_modules`, not root's). The target path uses the same
/// `<slot>/node_modules/<name>` shape that the on-disk hoist symlink
/// will point at, so [`PathBuf`] equality with
/// [`SymlinkDirectDependencies`](crate::SymlinkDirectDependencies)'s computed targets is exact.
#[must_use]
pub fn collect_public_hoist_targets(
    result: &crate::HoistResult,
    graph: &HashMap<PackageKey, crate::HoistGraphNode>,
    layout: &crate::VirtualStoreLayout,
    hoist_skipped: &HashSet<PackageKey>,
) -> BTreeMap<String, PathBuf> {
    let mut targets = BTreeMap::new();
    // Publicly-hoisted workspace packages land in root's
    // `node_modules/` too; their dedupe target is the project dir
    // the hoist symlink points at.
    for (alias, kind, project_dir) in &result.hoisted_workspace_aliases {
        if matches!(kind, pnpm_modules_yaml::HoistKind::Public) {
            targets.entry(alias.clone()).or_insert_with(|| project_dir.clone());
        }
    }
    for (node_id, alias_map) in &result.hoisted_dependencies_by_node_id {
        if hoist_skipped.contains(node_id) {
            continue;
        }
        let Some(node) = graph.get(node_id) else { continue };
        let dep_dir = layout.slot_dir(node_id).join("node_modules").join(node.name.to_string());
        add_public_aliases(&mut targets, alias_map, &dep_dir);
    }
    targets
}
/// First-wins: the traversal already chose one source per alias via its
/// `hoisted_aliases` claim. Multiple entries with the same alias would
/// be a hoister bug; preserve the first deterministically.
pub(super) fn add_public_aliases(
    targets: &mut BTreeMap<String, PathBuf>,
    alias_map: &HashMap<String, pnpm_modules_yaml::HoistKind>,
    dep_dir: &Path,
) {
    for (alias, kind) in alias_map {
        if matches!(kind, pnpm_modules_yaml::HoistKind::Public) {
            targets.entry(alias.clone()).or_insert_with(|| dep_dir.to_path_buf());
        }
    }
}
/// Pull the leading major-version digits out of a semver string like
/// `"22.11.0"`. Returns `None` if the leading token isn't parseable
/// as `u32`. Used to derive the engine-name string the
/// side-effects cache lookup expects without re-spawning
/// `node --version`.
#[must_use]
pub fn parse_major_from_version(version: &str) -> Option<u32> {
    let after_v = version.strip_prefix('v').unwrap_or(version);
    after_v.split('.').next()?.parse().ok()
}
/// Pull the `node@runtime:<version>` major out of a lockfile's
/// `snapshots:` map, if the project pinned a runtime Node.
///
/// The runtime resolver writes the pinned Node into the lockfile as a
/// snapshot with key `node@runtime:<version>`. The engine-name string
/// anchors the GVS hash and the side-effects-cache key prefix to that
/// pinned major instead of the host's own `node --version`. Scans the
/// snapshots with "first hit wins" semantics (the resolver rejects
/// workspaces with conflicting pins before they reach the lockfile).
///
/// Returns `None` when no importer pinned a runtime — callers should
/// then fall through to the host probe (`node --version` or the
/// cached `host_node`).
#[must_use]
pub fn find_runtime_node_major(
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
) -> Option<u32> {
    let snapshots = snapshots?;
    for key in snapshots.keys() {
        if key.suffix.prefix() != Prefix::Runtime {
            continue;
        }
        // Only `node@runtime:` feeds the Node-shaped engine string —
        // `bun@runtime:` and `deno@runtime:` exist as separate runtime
        // kinds. Scan for `node@runtime:` exclusively.
        if key.name.scope.is_some() || key.name.bare != "node" {
            continue;
        }
        // `Version::major` is `u64`; the major is small (<=99 in
        // practice), so the cast is lossless. The downstream
        // `engine_name` argument is `u32`.
        let major = key.suffix.version_semver()?.major;
        return Some(major as u32);
    }
    None
}
/// Read one snapshot's own `engines.runtime` Node pin from its
/// `dependencies` map. The resolver desugars `engines.runtime`
/// declared on a dep's manifest into
/// `dependencies.node: 'runtime:<version>'`.
///
/// Returns the bare major when this snapshot pins its own Node, or
/// `None` when it doesn't — callers should then fall back to the
/// install-wide pin / host probe via [`find_runtime_node_major`].
///
/// Per-snapshot resolution matters because the bin linker routes
/// lifecycle-script spawns for a pinning package through that
/// package's own downloaded Node. Anchoring the snapshot's GVS engine
/// hash to an install-wide value would produce the wrong
/// side-effects-cache key for cross-pinning installs.
#[must_use]
pub fn find_own_runtime_node_major(snapshot: &SnapshotEntry) -> Option<u32> {
    let deps = snapshot.dependencies.as_ref()?;
    for (alias, dep_ref) in deps {
        if alias.scope.is_some() || alias.bare != "node" {
            continue;
        }
        // `link:` deps have no version slot and can't carry a
        // `runtime:` pin — skip them.
        let Some(ver_peer) = dep_ref.ver_peer() else {
            continue;
        };
        if ver_peer.prefix() != Prefix::Runtime {
            continue;
        }
        // Same cast as `find_runtime_node_major` above; see the
        // comment there for why `u64 → u32` is lossless in practice.
        return Some(ver_peer.version_semver()?.major as u32);
    }
    None
}
