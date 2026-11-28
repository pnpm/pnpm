use crate::SkippedSnapshots;
use indexmap::IndexMap;
use pnpm_lockfile::{
    PackageKey,
    ProjectSnapshot,
    SnapshotEntry,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_patching::ExtendedPatchInfo;
use std::collections::{
    HashMap,
    HashSet,
};

/// Compute the dependency graph of packages that need building.
///
/// Only nodes whose subtree contains at least one build candidate appear in
/// the output. Snapshots not reachable from any importer are excluded.
///
/// `requires_build` is the per-snapshot map computed by the caller after
/// extraction (from each package's manifest scripts and presence of
/// `binding.gyp` / `.hooks/`).
///
/// `patches` is the per-snapshot lookup map produced by
/// `InstallFrozenLockfile::run`. Presence of a key here makes
/// the snapshot a build candidate even when `requires_build` is false.
///
/// `dependency_groups` are the groups this install includes, the
/// `--prod` / `--dev` / `--no-optional` selection. A package reachable
/// only through an excluded group never enters the graph, so its
/// lifecycle scripts do not run. `None` walks every group.
#[must_use]
pub fn build_graph(
    requires_build: &HashMap<PackageKey, bool>,
    patches: Option<&HashMap<PackageKey, ExtendedPatchInfo>>,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    importers: &HashMap<String, ProjectSnapshot>,
    dependency_groups: Option<&[DependencyGroup]>,
    skipped: &SkippedSnapshots,
) -> IndexMap<PackageKey, Vec<PackageKey>> {
    let children = build_children_map(snapshots);
    let root_dep_paths = collect_root_dep_paths(importers, snapshots, dependency_groups);

    let ctx = GetSubgraphCtx { children: &children, requires_build, patches, skipped };
    let nodes_to_build = subgraph_to_build(&root_dep_paths, &ctx);
    if nodes_to_build.is_empty() {
        return IndexMap::new();
    }
    let nodes_to_build_set: HashSet<&PackageKey> = nodes_to_build.iter().collect();

    nodes_to_build
        .iter()
        .map(|key| {
            let edges = children
                .get(key)
                .map(|cs| {
                    cs.iter()
                        .filter(|child| nodes_to_build_set.contains(child))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            (key.clone(), edges)
        })
        .collect()
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
        let mut child_keys: Vec<PackageKey> =
            [snap.dependencies.as_ref(), snap.optional_dependencies.as_ref()]
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
                .filter(|resolved| snapshots.contains_key(resolved))
                .collect();
        // Sorted for the same reason `collect_root_dep_paths` sorts
        // its output: the build graph lists its nodes in the order
        // `subgraph_to_build` walks them, so both the entry nodes and
        // every child list must be in a deterministic order for the
        // build graph to be reproducible.
        child_keys.sort_by_key(std::string::ToString::to_string);
        children.insert(key.clone(), child_keys);
    }
    children
}

/// Every group an importer can declare a direct dependency in.
/// `peerDependencies` is not one of them: a peer is satisfied out of the
/// dependent's own subtree, never installed as a root.
const ALL_ROOT_DEPENDENCY_GROUPS: [DependencyGroup; 3] =
    [DependencyGroup::Prod, DependencyGroup::Optional, DependencyGroup::Dev];

/// Gather snapshot keys for the direct dependencies importers declare in
/// `dependency_groups`, or in every group when that is `None`.
///
/// Keys whose constructed snapshot key is not in `snapshots` are dropped
/// silently (e.g. workspace links that are not separate packages).
fn collect_root_dep_paths(
    importers: &HashMap<String, ProjectSnapshot>,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    dependency_groups: Option<&[DependencyGroup]>,
) -> Vec<PackageKey> {
    let groups = dependency_groups.unwrap_or(&ALL_ROOT_DEPENDENCY_GROUPS);
    let mut seen: HashSet<PackageKey> = HashSet::new();
    // `link:` deps don't live in the virtual store (they're per-importer
    // directory symlinks) so they are not snapshot roots. For aliased deps,
    // the snapshot key uses the alias's own (name, suffix), not the
    // importer-map key.
    let mut roots: Vec<PackageKey> = importers
        .values()
        .flat_map(|snapshot| snapshot.dependencies_by_groups(groups.iter().copied()))
        .filter_map(|(name, spec)| spec.version.resolved_key(name))
        .filter(|key| snapshots.contains_key(key))
        .filter(|key| seen.insert(key.clone()))
        .collect();
    // The build graph lists its nodes in the order
    // [`subgraph_to_build`] walks them from these roots, which arrive
    // from `HashMap<_, ProjectSnapshot>` and
    // `HashMap<PkgName, ResolvedDependencySpec>` in an order that
    // changes from run to run. Sort by `PackageKey` string repr so the
    // build sequence is reproducible run to run.
    roots.sort_by_key(std::string::ToString::to_string);
    roots
}

/// What [`subgraph_to_build`] walks: each package's children, and what
/// makes a package a build candidate.
struct GetSubgraphCtx<'a> {
    children: &'a HashMap<PackageKey, Vec<PackageKey>>,
    requires_build: &'a HashMap<PackageKey, bool>,
    patches: Option<&'a HashMap<PackageKey, ExtendedPatchInfo>>,
    skipped: &'a SkippedSnapshots,
}

/// The packages of the build graph: every package the walk from
/// `entry_nodes` enters that is a build candidate or reaches one, in the
/// order the walk finishes them, each after the children it entered.
///
/// A node is a candidate when `requires_build` is set OR when an entry
/// for the peer-stripped key is present in `patches`, *unless* the
/// node is in `skipped`, in which case its virtual-store slot was never
/// created so neither the requires-build nor patch path can run.
///
/// A package is kept even when its only path to a candidate passes through a
/// package the walk entered from an earlier root, so that the edges
/// between kept packages order every candidate after each candidate it
/// reaches.
fn subgraph_to_build(entry_nodes: &[PackageKey], ctx: &GetSubgraphCtx<'_>) -> Vec<PackageKey> {
    let walked = walk_order(entry_nodes, ctx);
    let reaches_candidate = reaching_candidates(&walked, ctx);
    walked
        .iter()
        .filter(|key| reaches_candidate.contains(key))
        .cloned()
        .collect()
}

/// The packages of `walked` that are build candidates or reach one through
/// packages of `walked`. The search follows edges backwards from every
/// candidate and adds each package once, so it ends on a dependency cycle
/// and still finds every member of one that reaches a candidate.
fn reaching_candidates<'a>(
    walked: &'a [PackageKey],
    ctx: &GetSubgraphCtx<'_>,
) -> HashSet<&'a PackageKey> {
    let parents = parents_within(walked, ctx);
    let mut reaching: HashSet<&PackageKey> = walked
        .iter()
        .filter(|key| node_builds(key, ctx))
        .collect();
    let mut pending: Vec<&PackageKey> = reaching.iter().copied().collect();
    while let Some(node) = pending.pop() {
        for &parent in parents.get(node).into_iter().flatten() {
            if reaching.insert(parent) {
                pending.push(parent);
            }
        }
    }
    reaching
}

/// Each package of `walked` mapped to the packages of `walked` that depend
/// on it.
fn parents_within<'a>(
    walked: &'a [PackageKey],
    ctx: &GetSubgraphCtx<'_>,
) -> HashMap<&'a PackageKey, Vec<&'a PackageKey>> {
    let walked_set: HashSet<&PackageKey> = walked.iter().collect();
    let mut parents: HashMap<&PackageKey, Vec<&PackageKey>> = HashMap::new();
    for parent in walked {
        for child in ctx.children
            .get(parent)
            .into_iter()
            .flatten()
        {
            if let Some(&child) = walked_set.get(child) {
                parents
                    .entry(child)
                    .or_default()
                    .push(parent);
            }
        }
    }
    parents
}

/// Every package the walk from `entry_nodes` enters, each listed once the
/// walk has entered all of its children, with the walk's frames on a
/// stack of its own rather than one per dependency on the thread's.
fn walk_order(entry_nodes: &[PackageKey], ctx: &GetSubgraphCtx<'_>) -> Vec<PackageKey> {
    let mut walked: HashSet<PackageKey> = HashSet::new();
    let mut order: Vec<PackageKey> = Vec::new();
    // Each frame holds a package and the index of its next child.
    let mut frames: Vec<(&PackageKey, usize)> = Vec::new();
    for entry in entry_nodes {
        if enters_build_graph(entry, ctx, &mut walked) {
            walked.insert(entry.clone());
            frames.push((entry, 0));
        }
        while let Some(frame) = frames.last_mut() {
            let (node, next) = *frame;
            frame.1 += 1;
            match ctx.children
                .get(node)
                .and_then(|children| children.get(next))
            {
                Some(child) if enters_build_graph(child, ctx, &mut walked) => {
                    walked.insert(child.clone());
                    frames.push((child, 0));
                }
                Some(_) => {}
                None => {
                    order.push(node.clone());
                    frames.pop();
                }
            }
        }
    }
    order
}

/// Whether the walk descends through this node at all. A node outside the
/// graph is already in `node_modules`, and one already walked was reached
/// through an earlier root.
///
/// A skipped snapshot never had its virtual-store slot created, so neither
/// requires-build nor a configured patch can produce work. A skipped depPath
/// is dropped from the build graph entirely: a child reachable only via a
/// skipped edge doesn't enter the build graph either. Gated before the
/// walk enters the node so a skipped optional doesn't drag its transitive deps into the
/// walk via an edge that should not be followed, which is why a skipped node
/// is still marked walked.
fn enters_build_graph(
    dep_path: &PackageKey,
    ctx: &GetSubgraphCtx<'_>,
    walked: &mut HashSet<PackageKey>,
) -> bool {
    if !ctx.children.contains_key(dep_path) || walked.contains(dep_path) {
        return false;
    }
    if ctx.skipped.contains(dep_path) {
        walked.insert(dep_path.clone());
        return false;
    }
    true
}

fn node_builds(dep_path: &PackageKey, ctx: &GetSubgraphCtx<'_>) -> bool {
    ctx.requires_build
        .get(dep_path)
        .copied()
        .unwrap_or(false)
        || ctx.patches.is_some_and(|patches| patches.contains_key(&dep_path.without_peer()))
}

#[cfg(test)]
mod tests;
