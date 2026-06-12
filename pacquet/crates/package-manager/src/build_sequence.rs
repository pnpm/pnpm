use crate::{
    SkippedSnapshots,
    graph_sequencer::{GraphSequencerResult, graph_sequencer},
};
use pacquet_lockfile::{PackageKey, ProjectSnapshot, SnapshotEntry};
use pacquet_patching::ExtendedPatchInfo;
use std::collections::{HashMap, HashSet};

/// Compute topologically ordered chunks of packages that need building.
///
/// Ports `buildSequence` from
/// `https://github.com/pnpm/pnpm/blob/80037699fb/building/during-install/src/buildSequence.ts`.
///
/// The returned chunks are ordered children-first: every chunk may safely
/// run only after every preceding chunk has finished. Members of the same
/// chunk are independent and could run concurrently (pacquet currently runs
/// them sequentially).
///
/// Only nodes whose subtree contains at least one build candidate appear in
/// the output. Snapshots not reachable from any importer are excluded —
/// matching pnpm's `getSubgraphToBuild` walk.
///
/// `requires_build` is the per-snapshot map computed by the caller after
/// extraction (from each package's manifest scripts and presence of
/// `binding.gyp` / `.hooks/`). Mirrors the role of `node.requiresBuild`
/// upstream, which the worker computes from the extracted package contents.
///
/// `patches` is the per-snapshot lookup map produced by
/// `InstallFrozenLockfile::run` from
/// [`pacquet_patching::resolve_and_group`] + per-snapshot
/// [`pacquet_patching::get_patch_info`]: keys are peer-stripped
/// [`PackageKey`]s, values are the matched
/// [`pacquet_patching::ExtendedPatchInfo`]. `None` when no
/// `patchedDependencies` is configured. Presence of a key here
/// mirrors upstream's `node.patch != null` and makes the snapshot
/// a build candidate even when `requires_build` is false. Mirrors
/// upstream's
/// [`getSubgraphToBuild`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/buildSequence.ts#L40-L67).
pub fn build_sequence(
    requires_build: &HashMap<PackageKey, bool>,
    patches: Option<&HashMap<PackageKey, ExtendedPatchInfo>>,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    importers: &HashMap<String, ProjectSnapshot>,
    skipped: &SkippedSnapshots,
) -> Vec<Vec<PackageKey>> {
    let children = build_children_map(snapshots);
    let root_dep_paths = collect_root_dep_paths(importers, snapshots);

    let mut nodes_to_build_set: HashSet<PackageKey> = HashSet::new();
    let mut nodes_to_build: Vec<PackageKey> = Vec::new();
    let mut walked: HashSet<PackageKey> = HashSet::new();
    let ctx = GetSubgraphCtx { children: &children, requires_build, patches, skipped };
    get_subgraph_to_build(
        &root_dep_paths,
        &ctx,
        &mut nodes_to_build_set,
        &mut nodes_to_build,
        &mut walked,
    );

    if nodes_to_build.is_empty() {
        return Vec::new();
    }

    let filtered_graph: HashMap<PackageKey, Vec<PackageKey>> = nodes_to_build
        .iter()
        .map(|key| {
            let edges = children
                .get(key)
                .map(|cs| {
                    cs.iter().filter(|child| nodes_to_build_set.contains(child)).cloned().collect()
                })
                .unwrap_or_default();
            (key.clone(), edges)
        })
        .collect();

    let GraphSequencerResult { chunks, safe, .. } =
        graph_sequencer(&filtered_graph, &nodes_to_build);
    if !safe {
        tracing::warn!(
            target: "pacquet::build",
            "dependency cycle detected while computing build order; \
             packages inside the cycle will run in arbitrary order",
        );
    }
    chunks
}

/// Build the `node → children` adjacency map from the snapshot map.
///
/// Children are the resolved snapshot keys of `dependencies` and
/// `optional_dependencies`. Edges to keys not present in the snapshot map
/// are dropped (matches pnpm: missing nodes mean the dependency was already
/// in `node_modules` and not part of this install's graph).
fn build_children_map(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
) -> HashMap<PackageKey, Vec<PackageKey>> {
    let mut children: HashMap<PackageKey, Vec<PackageKey>> =
        HashMap::with_capacity(snapshots.len());
    for (key, snap) in snapshots {
        let mut child_keys: Vec<PackageKey> = Vec::new();
        for deps in
            [snap.dependencies.as_ref(), snap.optional_dependencies.as_ref()].into_iter().flatten()
        {
            for (alias, dep_ref) in deps {
                let Some(resolved) = dep_ref.resolve(alias) else {
                    continue;
                };
                if snapshots.contains_key(&resolved) {
                    child_keys.push(resolved);
                }
            }
        }
        // Sort for the same reason `collect_root_dep_paths` sorts
        // its output: `get_subgraph_to_build` walks children in
        // sequence, and a shared transitive descendant gets trimmed
        // off whichever sibling visits it second. Both the entry
        // nodes and every child list must be in a deterministic
        // order for the build sequence to be reproducible.
        child_keys.sort_by_key(std::string::ToString::to_string);
        children.insert(key.clone(), child_keys);
    }
    children
}

/// Gather snapshot keys for every direct dependency declared by an importer.
///
/// Iterates `dependencies`, `devDependencies`, and `optionalDependencies` of
/// every importer. Keys whose constructed snapshot key is not in `snapshots`
/// are dropped silently (e.g. workspace links that are not separate packages).
fn collect_root_dep_paths(
    importers: &HashMap<String, ProjectSnapshot>,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
) -> Vec<PackageKey> {
    let mut roots: Vec<PackageKey> = Vec::new();
    let mut seen: HashSet<PackageKey> = HashSet::new();
    for snapshot in importers.values() {
        for map in [
            snapshot.dependencies.as_ref(),
            snapshot.optional_dependencies.as_ref(),
            snapshot.dev_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            for (name, spec) in map {
                // `link:` deps don't live in the virtual store —
                // they're per-importer directory symlinks — so they
                // are not snapshot roots. Mirrors upstream's
                // `if (depPath.startsWith('link:')) continue` at
                // build-time in
                // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/index.ts>.
                // For aliased deps, the snapshot key uses the alias's
                // own (name, suffix), not the importer-map key.
                let Some(key) = spec.version.resolved_key(name) else {
                    continue;
                };
                if !snapshots.contains_key(&key) {
                    continue;
                }
                if seen.insert(key.clone()) {
                    roots.push(key);
                }
            }
        }
    }
    // [`get_subgraph_to_build`] is order-sensitive (a node walked
    // first via root A may mark a shared child as already-walked, so
    // a second root B sharing that child gets trimmed). Upstream's
    // input arrives in JS-object insertion order; pacquet sources
    // these from `HashMap<_, ProjectSnapshot>` and
    // `HashMap<PkgName, ResolvedDependencySpec>`, so iteration order
    // is non-deterministic. Sort by `PackageKey` string repr so the
    // build sequence (and the trim behavior) is reproducible run to
    // run. Long-term fix is to preserve lockfile declaration order
    // via `IndexMap`; until then, an alphabetical sort is enough to
    // make the build path deterministic.
    roots.sort_by_key(std::string::ToString::to_string);
    roots
}

/// Per-walk invariant inputs to [`get_subgraph_to_build`]. Bundled
/// into a struct so the recursive call doesn't have to thread eight
/// arguments through every level — the three mutable accumulators
/// stay as `&mut` params (one each because they're typed differently).
struct GetSubgraphCtx<'a> {
    children: &'a HashMap<PackageKey, Vec<PackageKey>>,
    requires_build: &'a HashMap<PackageKey, bool>,
    patches: Option<&'a HashMap<PackageKey, ExtendedPatchInfo>>,
    skipped: &'a SkippedSnapshots,
}

/// Walk the dep graph from `entry_nodes`, filling `nodes_to_build` with
/// packages whose subtree (including themselves) contains a build candidate.
///
/// Ports `getSubgraphToBuild` from
/// `https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/buildSequence.ts`.
/// A node is a candidate when `requires_build` is set OR when an entry
/// for the peer-stripped key is present in `patches` (mirrors
/// upstream's `node.requiresBuild || node.patch != null`) — *unless*
/// the node is in `skipped`, in which case its virtual-store slot
/// was never created so neither the requires-build nor patch path
/// can run. Mirrors pnpm's `lockfileToDepGraph` flow where skipped
/// snapshots never enter the build graph at all (the patch lookup
/// upstream walks `pkgGraph[depPath]?.patch` and `depGraph` itself
/// excludes skipped nodes).
///
/// Returns whether *any* of the entry nodes (or their subtrees) needs to build.
fn get_subgraph_to_build(
    entry_nodes: &[PackageKey],
    ctx: &GetSubgraphCtx<'_>,
    nodes_to_build_set: &mut HashSet<PackageKey>,
    nodes_to_build: &mut Vec<PackageKey>,
    walked: &mut HashSet<PackageKey>,
) -> bool {
    let mut current_should_be_built = false;
    for dep_path in entry_nodes {
        if !ctx.children.contains_key(dep_path) {
            continue; // already in node_modules / not part of this graph
        }
        if walked.contains(dep_path) {
            continue;
        }

        // A skipped snapshot never had its virtual-store slot
        // created, so neither requires-build nor a configured
        // patch can produce work. Mirrors pnpm's `lockfileToDepGraph`
        // flow where skipped depPaths are dropped from `depGraph`
        // entirely: a child reachable only via a skipped edge
        // doesn't enter the build graph either. Gate *before*
        // recursion so a skipped optional doesn't drag its
        // transitive deps into the walk via an edge pnpm wouldn't
        // see.
        //
        // A descendant of a skipped node that's ALSO reachable from
        // a non-skipped root still gets visited normally on that
        // other branch, because we don't poison `walked` for the
        // child here — we just skip this edge.
        if ctx.skipped.contains(dep_path) {
            walked.insert(dep_path.clone());
            continue;
        }

        walked.insert(dep_path.clone());

        let child_paths = ctx.children.get(dep_path).cloned().unwrap_or_default();
        let child_should_be_built =
            get_subgraph_to_build(&child_paths, ctx, nodes_to_build_set, nodes_to_build, walked);

        let needs_build = ctx.requires_build.get(dep_path).copied().unwrap_or(false);
        let has_patch = ctx.patches.is_some_and(|p| p.contains_key(&dep_path.without_peer()));

        if child_should_be_built || needs_build || has_patch {
            if nodes_to_build_set.insert(dep_path.clone()) {
                nodes_to_build.push(dep_path.clone());
            }
            current_should_be_built = true;
        }
    }
    current_should_be_built
}

#[cfg(test)]
mod tests;
