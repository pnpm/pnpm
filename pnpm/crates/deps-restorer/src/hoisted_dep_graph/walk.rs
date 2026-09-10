use super::{
    DepHierarchy, DependenciesGraph, DependenciesGraphNode, DirectDependenciesByImporterId,
    HoistedDepGraphError, LockfileToDepGraphResult, LockfileToHoistedDepGraphOptions,
    installability_skip, lookup_package_metadata, package_present_at,
    path_relative_to_lockfile_dir, resolution_changed_at,
};
use crate::safe_join_modules_dir::safe_join_modules_dir;
use indexmap::IndexSet;
use pnpm_deps_path::get_pkg_id_with_patch_hash;
use pnpm_lockfile::{Lockfile, LockfileResolution, PackageKey, PkgIdWithPatchHash};
use pnpm_modules_yaml::DepPath;
use pnpm_real_hoist::{HoisterResult, RcByPtr};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

impl WalkState<'_> {
    /// Pass 2 — fill in each node's `children` map from the
    /// now-complete `pkg_locations_by_pkg_id`, then assemble the result.
    ///
    /// The walk intentionally leaves `children` empty: every sibling and
    /// descendant of a node must have its directory recorded in
    /// `pkg_locations_by_pkg_id` before any node resolves its children,
    /// so the location index is complete by the time children are
    /// computed. The simplest way to preserve that invariant is to
    /// insert everything first and resolve children second.
    pub(super) fn into_result(
        mut self,
        root_hierarchy: DepHierarchy,
    ) -> Result<LockfileToDepGraphResult, HoistedDepGraphError> {
        fill_children(&mut self.graph, &self.pkg_locations_by_pkg_id, self.lockfile)?;

        // The hoister produced a children order; the directory keys in
        // `root_hierarchy` follow it, and
        // `direct_dependencies_by_importer_id["."]` is built from that
        // order.
        let mut direct_dependencies_by_importer_id: DirectDependenciesByImporterId =
            BTreeMap::new();
        direct_dependencies_by_importer_id.insert(
            Lockfile::ROOT_IMPORTER_KEY.to_string(),
            root_direct_deps(&root_hierarchy, &self.graph),
        );

        // `link:` entries are skipped — they don't enter the hoist tree
        // and have no `pkg_locations` entry. The install pipeline handles
        // them via [`crate::SymlinkDirectDependencies`]'s `link_only` pass
        // after the hoisted linker runs.
        for importer_id in self.per_importer_direct_deps.keys() {
            let Some(importer) = self.lockfile.importers.get(importer_id) else { continue };
            direct_dependencies_by_importer_id.insert(
                importer_id.clone(),
                importer_direct_deps(importer, &self.pkg_locations_by_pkg_id),
            );
        }

        // Hierarchy: one entry per importer root. Root importer gets
        // `lockfile_dir`; non-root workspace importers get
        // `<lockfile_dir>/<importer_id>` — the linker walks each
        // importer's subtree under its own root.
        let mut hierarchy = BTreeMap::new();
        hierarchy.insert(self.opts.lockfile_dir.clone(), root_hierarchy);
        hierarchy.extend(self.per_importer_hierarchies);

        Ok(LockfileToDepGraphResult {
            graph: self.graph,
            direct_dependencies_by_importer_id,
            hierarchy,
            hoisted_locations: self.hoisted_locations,
            symlinked_direct_dependencies_by_importer_id: DirectDependenciesByImporterId::new(),
            prev_graph: None,
            injection_targets_by_dep_path: self.injection_targets_by_dep_path,
            skipped: self.skipped,
        })
    }
}
/// The root importer's direct dependencies, in the children order the
/// hoister produced.
pub(super) fn root_direct_deps(
    root_hierarchy: &DepHierarchy,
    graph: &DependenciesGraph,
) -> BTreeMap<String, PathBuf> {
    let mut direct_deps = BTreeMap::new();
    for child_dir in root_hierarchy.0.keys() {
        if let Some(alias) = graph.get(child_dir).and_then(|node| node.alias.as_deref()) {
            direct_deps.insert(alias.to_string(), child_dir.clone());
        }
    }
    direct_deps
}
/// One non-root importer's direct dependencies, read off its declared
/// lockfile entries rather than off its hoist-tree node: the hoister
/// moves dedupe-able deps up to root, leaving the workspace node's
/// children empty even when the importer declared those deps. The first
/// recorded location of each resolved snapshot wins.
pub(super) fn importer_direct_deps(
    importer: &pnpm_lockfile::ProjectSnapshot,
    pkg_locations_by_pkg_id: &BTreeMap<String, Vec<PathBuf>>,
) -> BTreeMap<String, PathBuf> {
    let mut direct_deps = BTreeMap::new();
    for dep_map in [
        importer.dependencies.as_ref(),
        importer.dev_dependencies.as_ref(),
        importer.optional_dependencies.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        for (alias, spec) in dep_map {
            // For an aliased dep the snapshot key uses the alias's own
            // (name, suffix); for a regular dep it's `(alias, version)`.
            let Some(dep_key) = spec.version.resolved_key(alias) else { continue };
            if let Some(first) = pkg_locations_by_pkg_id
                .get(&pnpm_real_hoist::pkg_id(&dep_key))
                .and_then(|locations| locations.first())
            {
                direct_deps.insert(alias.to_string(), first.clone());
            }
        }
    }
    direct_deps
}
/// Second walker pass: with every node's directory already in
/// `pkg_locations`, resolve each graph node's `children: alias →
/// dir` map by looking up the node's snapshot in the lockfile.
pub(super) fn fill_children(
    graph: &mut DependenciesGraph,
    pkg_locations: &BTreeMap<String, Vec<PathBuf>>,
    lockfile: &Lockfile,
) -> Result<(), HoistedDepGraphError> {
    let dirs: Vec<PathBuf> = graph.keys().cloned().collect();
    for dir in dirs {
        let reference = graph[&dir].dep_path.as_str().to_string();
        let pkg_key: PackageKey = match reference.parse() {
            Ok(key) => key,
            Err(source) => {
                return Err(HoistedDepGraphError::BadReference { reference, source });
            }
        };
        let snapshot = lockfile.snapshots.as_ref().and_then(|m| m.get(&pkg_key));
        let children = compute_children(snapshot, pkg_locations);
        if let Some(node) = graph.get_mut(&dir) {
            node.children = children;
        }
    }
    Ok(())
}
/// Mutable scratch space the recursive walker threads through
/// every level. Borrowing the lockfile + `lockfile_dir` + opts up
/// front avoids passing four separate arguments. `skipped` is
/// owned (cloned from `opts.skipped`) because the walker mutates
/// it — every dep that fails the installability check gets added.
pub(super) struct WalkState<'a> {
    pub(super) lockfile: &'a Lockfile,
    pub(super) lockfile_dir: &'a Path,
    pub(super) opts: &'a LockfileToHoistedDepGraphOptions<'a>,
    /// The graph the current lockfile produces, when there is one. Only
    /// [`walk_dep`]'s presence check reads it, to see whether the
    /// package the previous install put at a directory resolves the same
    /// way as the one going there now. `None` on the fresh-lockfile
    /// path, which has no current lockfile to walk.
    pub(super) prev_graph: Option<&'a DependenciesGraph>,
    pub(super) skipped: BTreeSet<String>,
    pub(super) graph: DependenciesGraph,
    /// Records every directory each package landed in, in visit
    /// order. The first entry wins for parent → child wiring.
    ///
    /// Keyed by [`pnpm_real_hoist::pkg_id`], not the snapshot key:
    /// the hoister collapses every peer variant of one package
    /// version onto a single node, so only the first-seen variant's
    /// key reaches this walk. Sharing the hoister's own identity
    /// function is what lets an edge declared against any other
    /// variant still find that node's directory.
    pub(super) pkg_locations_by_pkg_id: BTreeMap<String, Vec<PathBuf>>,
    pub(super) hoisted_locations: BTreeMap<String, Vec<String>>,
    pub(super) injection_targets_by_dep_path: BTreeMap<String, Vec<PathBuf>>,
    /// Per-non-root-importer hierarchy emitted while walking
    /// `Workspace`-kind nodes. Outer key is the importer's root
    /// directory (`<lockfile_dir>/<importer_id>`). Folded into
    /// [`LockfileToDepGraphResult::hierarchy`] alongside the root
    /// importer's hierarchy by [`build_dep_graph`](crate::hoisted_dep_graph::build_dep_graph).
    pub(super) per_importer_hierarchies: BTreeMap<PathBuf, DepHierarchy>,
    /// Per-non-root-importer direct dependencies emitted while
    /// walking `Workspace`-kind nodes. Outer key is the importer
    /// id from the lockfile (e.g. `packages/foo`). Folded into
    /// [`LockfileToDepGraphResult::direct_dependencies_by_importer_id`]
    /// alongside the root importer's entry by [`build_dep_graph`](crate::hoisted_dep_graph::build_dep_graph).
    pub(super) per_importer_direct_deps: DirectDependenciesByImporterId,
}
/// Recursive walker over `HoisterResult.dependencies`. Skips the
/// store-fetch / installability path; here the walker only computes
/// node identity, location, children, and hoisted-location records.
///
/// No cycle detection — the walk trusts the hoister to produce a
/// DAG. The hoister's own cyclic-input tests pin that property.
pub(super) fn walk_deps(
    state: &mut WalkState<'_>,
    modules: &Path,
    deps: &IndexSet<RcByPtr<HoisterResult>>,
) -> Result<DepHierarchy, HoistedDepGraphError> {
    let mut hierarchy: BTreeMap<PathBuf, DepHierarchy> = BTreeMap::new();
    for dep in deps {
        if let Some((dir, inner_hierarchy)) = walk_dep(state, modules, dep)? {
            hierarchy.insert(dir, inner_hierarchy);
        }
    }
    Ok(DepHierarchy(hierarchy))
}
/// One node of [`walk_deps`]. `None` when the node contributes nothing
/// to the parent's hierarchy: a workspace importer, a link placeholder,
/// or a package the installability filter skipped.
pub(super) fn walk_dep(
    state: &mut WalkState<'_>,
    modules: &Path,
    dep: &RcByPtr<HoisterResult>,
) -> Result<Option<(PathBuf, DepHierarchy)>, HoistedDepGraphError> {
    // The hoister keeps every absorbed reference; the first
    // (alphabetically smallest) is the canonical depPath for this
    // node's location.
    let Some(reference) = dep.0.references.borrow().iter().next().cloned() else {
        return Ok(None);
    };

    if state.skipped.contains(&reference) {
        return Ok(None);
    }

    if let Some(importer_id) = reference.strip_prefix("workspace:") {
        walk_workspace_importer(state, dep, importer_id)?;
        return Ok(None);
    }

    let Some(resolved) = resolve_reference(state, &reference)? else {
        return Ok(None);
    };
    let optional = resolved.snapshot.is_some_and(|snapshot| snapshot.optional);

    if installability_skip(state, &resolved.pkg_key, resolved.metadata, optional)? {
        state.skipped.insert(reference);
        return Ok(None);
    }

    let dir = safe_join_modules_dir(modules, &dep.0.name)?;
    let dep_location = path_relative_to_lockfile_dir(&dir, state.lockfile_dir);
    let present = package_is_reusable(state, &resolved, &reference, &dep_location, modules, &dir);

    // Insert *before* recursing (insert + push to `pkg_locations`, then
    // recurse) so every node's location is recorded ahead of any child
    // that needs to resolve to it. `children` is filled in by
    // `fill_children` after the whole walk is done.
    state.graph.insert(
        dir.clone(),
        graph_node(dep, &reference, &resolved, optional, present, &dir, modules),
    );
    state
        .pkg_locations_by_pkg_id
        .entry(pnpm_real_hoist::pkg_id(&resolved.pkg_key))
        .or_default()
        .push(dir.clone());

    // Directory resolutions are injected workspace packages. Record
    // every dir an injected dep lands in for the post-install re-mirror
    // step, so a future re-mirror pass has the input it needs.
    if let LockfileResolution::Directory(_) = &resolved.metadata.resolution {
        state.injection_targets_by_dep_path.entry(reference.clone()).or_default().push(dir.clone());
    }

    let hierarchy = walk_deps(state, &dir.join("node_modules"), &dep.0.dependencies.borrow())?;

    // `hoistedLocations` is pushed AFTER the recursion. The
    // pre-recursion sites that mutate state are for graph/index
    // identity; this one is the user-visible location list that the
    // linker consumes.
    state.hoisted_locations.entry(reference).or_default().push(dep_location);
    Ok(Some((dir, hierarchy)))
}
/// Mutable directory dependencies and patches need a fresh copy. Other packages must match
/// both the previous recorded location and the on-disk manifest version, and retain their resolution.
pub(super) fn package_is_reusable(
    state: &WalkState<'_>,
    resolved: &ResolvedReference<'_>,
    reference: &str,
    dep_location: &str,
    modules: &Path,
    dir: &Path,
) -> bool {
    let expected_version = resolved
        .metadata
        .version
        .clone()
        .unwrap_or_else(|| resolved.pkg_key.suffix.version().to_string());
    !state.opts.force
        && !matches!(resolved.metadata.resolution, LockfileResolution::Directory(_))
        && !reference.contains("(patch_hash=")
        && state.opts.current_hoisted_locations.is_some_and(|locations| {
            locations.get(reference).is_some_and(|dirs| dirs.iter().any(|dir| dir == dep_location))
        })
        && !resolution_changed_at(state.prev_graph, dir, &resolved.metadata.resolution)
        && package_present_at(modules, dir, &expected_version)
}
/// Workspace-kind hoister children are non-root workspace importers.
/// Recurse into their (post-hoist, often-empty) dependencies under
/// `<lockfile_dir>/<importer_id>/node_modules` to capture any deps the
/// hoister couldn't move up — those become nested entries in the
/// per-importer hierarchy. The workspace node itself is *not* added to
/// the graph or to the parent's hierarchy: it has no package contents
/// to import. Per-importer `direct_dependencies_by_importer_id` is
/// computed in [`WalkState::into_result`] from the lockfile (not from
/// the hoister tree) because hoisted siblings don't appear in the
/// workspace node's children.
pub(super) fn walk_workspace_importer(
    state: &mut WalkState<'_>,
    dep: &RcByPtr<HoisterResult>,
    importer_id: &str,
) -> Result<(), HoistedDepGraphError> {
    let importer_root = state.lockfile_dir.join(importer_id);
    let importer_hierarchy =
        walk_deps(state, &importer_root.join("node_modules"), &dep.0.dependencies.borrow())?;
    state.per_importer_hierarchies.insert(importer_root, importer_hierarchy);
    // Reserve the importer's slot so [`WalkState::into_result`]'s
    // post-walk loop knows the importer was visited, even when it ends
    // up with zero direct deps.
    state.per_importer_direct_deps.entry(importer_id.to_string()).or_default();
    Ok(())
}
/// The lockfile's metadata and snapshot for a hoister reference. `None`
/// for a link / external placeholder the wrapper strips, which the
/// walker skips.
pub(super) struct ResolvedReference<'l> {
    pkg_key: PackageKey,
    metadata: &'l pnpm_lockfile::PackageMetadata,
    snapshot: Option<&'l pnpm_lockfile::SnapshotEntry>,
}
pub(super) fn resolve_reference<'l>(
    state: &WalkState<'l>,
    reference: &str,
) -> Result<Option<ResolvedReference<'l>>, HoistedDepGraphError> {
    let pkg_key: PackageKey = match reference.parse() {
        Ok(key) => key,
        Err(source) => {
            return Err(HoistedDepGraphError::BadReference {
                reference: reference.to_string(),
                source,
            });
        }
    };
    let Some(metadata) = lookup_package_metadata(state.lockfile, &pkg_key) else {
        return Ok(None);
    };
    let snapshot = state.lockfile.snapshots.as_ref().and_then(|snapshots| snapshots.get(&pkg_key));
    Ok(Some(ResolvedReference { pkg_key, metadata, snapshot }))
}
pub(super) fn graph_node(
    dep: &RcByPtr<HoisterResult>,
    reference: &str,
    resolved: &ResolvedReference<'_>,
    optional: bool,
    present: bool,
    dir: &Path,
    modules: &Path,
) -> DependenciesGraphNode {
    DependenciesGraphNode {
        alias: Some(dep.0.name.clone()),
        dep_path: DepPath::from(reference.to_string()),
        // `pkgIdWithPatchHash` strips peer-graph hashes but keeps
        // `(patch_hash=...)`.
        pkg_id_with_patch_hash: PkgIdWithPatchHash::from(
            get_pkg_id_with_patch_hash(&resolved.pkg_key.to_string()).to_string(),
        ),
        dir: dir.to_path_buf(),
        modules: modules.to_path_buf(),
        children: BTreeMap::new(),
        name: resolved.pkg_key.name.to_string(),
        version: resolved.pkg_key.suffix.version().to_string(),
        optional,
        optional_dependencies: resolved
            .snapshot
            .and_then(|snap| snap.optional_dependencies.as_ref())
            .map(|map| map.keys().map(std::string::ToString::to_string).collect())
            .unwrap_or_default(),
        has_bin: resolved.metadata.has_bin.unwrap_or(false),
        has_bundled_dependencies: resolved.metadata.bundled_dependencies.is_some(),
        patch: None,
        resolution: resolved.metadata.resolution.clone(),
        present,
    }
}
/// Compute the `children: alias → dir` map for a node: look up
/// every direct (and optional, with `include` always on here) dep
/// of the snapshot, resolve it to its snapshot key via
/// `SnapshotDepRef::resolve`, and take the first recorded
/// location.
pub(super) fn compute_children(
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
    pkg_locations: &BTreeMap<String, Vec<PathBuf>>,
) -> BTreeMap<String, PathBuf> {
    let mut children: BTreeMap<String, PathBuf> = BTreeMap::new();
    let Some(snapshot) = snapshot else { return children };

    let dep_iter = snapshot
        .dependencies
        .iter()
        .flatten()
        .chain(snapshot.optional_dependencies.iter().flatten());
    for (alias_name, dep_ref) in dep_iter {
        // `link:` deps return `None` here — they live outside the
        // virtual store and don't show up in `pkg_locations`.
        let Some(child_key) = dep_ref.resolve(alias_name) else {
            continue;
        };
        if let Some(locations) = pkg_locations.get(&pnpm_real_hoist::pkg_id(&child_key))
            && let Some(first) = locations.first()
        {
            children.insert(alias_name.to_string(), first.clone());
        }
    }
    children
}
