use super::{CheckCache, cached_check};
use pnpm_lockfile::{PackageKey, PackageMetadata, ProjectSnapshot, SnapshotEntry};
use pnpm_package_is_installable::{InstallabilityError, InstallabilityOptions};
use std::collections::{HashMap, HashSet, VecDeque};

/// Edge classification produced by [`walk_lockfile_edges`].
pub(super) struct LockfileEdgeReach<'lock> {
    /// Snapshots reachable from any importer through any edge chain,
    /// including chains through skipped parents.
    pub(super) reachable: HashSet<&'lock PackageKey>,
    /// Skip candidates (incompatible when checked as optional) with a
    /// non-optional inbound edge from an installed source — an
    /// importer or a non-skipped snapshot. The fail / warn dispatch
    /// wins for these.
    pub(super) required: HashSet<&'lock PackageKey>,
}
/// Walk the lockfile graph from every importer and classify each
/// snapshot's inbound edges for the per-edge dispatch in
/// [`compute_skipped_snapshots`](crate::installability::compute_skipped_snapshots).
pub(super) fn walk_lockfile_edges<'lock>(
    importers: &HashMap<String, ProjectSnapshot>,
    snapshots: &'lock HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    base_options: &InstallabilityOptions<'_>,
    check_cache: &mut CheckCache,
) -> Result<LockfileEdgeReach<'lock>, Box<InstallabilityError>> {
    let reachable = reachable_snapshots(importers, snapshots);
    let required = required_snapshots(importers, snapshots, packages, base_options, check_cache)?;
    Ok(LockfileEdgeReach { reachable, required })
}
/// Every snapshot an importer can reach through any edge chain,
/// including chains through skipped parents.
pub(super) fn reachable_snapshots<'lock>(
    importers: &HashMap<String, ProjectSnapshot>,
    snapshots: &'lock HashMap<PackageKey, SnapshotEntry>,
) -> HashSet<&'lock PackageKey> {
    let mut reachable: HashSet<&'lock PackageKey> = HashSet::new();
    let mut queue: VecDeque<&'lock PackageKey> = VecDeque::new();
    for importer in importers.values() {
        enqueue_reachable(importer_edges(importer), snapshots, &mut reachable, &mut queue);
    }
    while let Some(key) = queue.pop_front() {
        enqueue_reachable(snapshot_edges(&snapshots[key]), snapshots, &mut reachable, &mut queue);
    }
    reachable
}
pub(super) fn enqueue_reachable<'lock>(
    edges: impl Iterator<Item = (PackageKey, bool)>,
    snapshots: &'lock HashMap<PackageKey, SnapshotEntry>,
    reachable: &mut HashSet<&'lock PackageKey>,
    queue: &mut VecDeque<&'lock PackageKey>,
) {
    for (target, _) in edges {
        if let Some((key, _)) = snapshots.get_key_value(&target)
            && reachable.insert(key)
        {
            queue.push_back(key);
        }
    }
}
/// Propagate installed-ness down from the importers. A skip candidate
/// is installed only once a non-optional edge from an installed source
/// reaches it; every other snapshot is installed as soon as any edge
/// from an installed source does. Edges out of a never-installed
/// candidate are not expanded, so a subtree behind a skipped parent
/// stays skippable no matter what edge kinds it uses internally.
pub(super) fn required_snapshots<'lock>(
    importers: &HashMap<String, ProjectSnapshot>,
    snapshots: &'lock HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    base_options: &InstallabilityOptions<'_>,
    check_cache: &mut CheckCache,
) -> Result<HashSet<&'lock PackageKey>, Box<InstallabilityError>> {
    let mut propagation = EdgePropagation { installed: HashSet::new(), required: HashSet::new() };
    let mut pending: VecDeque<(&'lock PackageKey, bool)> = importers
        .values()
        .flat_map(importer_edges)
        .filter_map(|(target, edge_optional)| {
            snapshots.get_key_value(&target).map(|(key, _)| (key, edge_optional))
        })
        .collect();
    while let Some((key, edge_optional)) = pending.pop_front() {
        // A node's classification is final once installed: `required`
        // is only ever entered on the installing edge, so later
        // inbound edges can skip the candidate check entirely.
        if propagation.installed.contains(key) {
            continue;
        }
        let skip_candidate =
            is_skip_candidate(&key.without_peer(), packages, base_options, check_cache)?;
        if propagation.install(key, edge_optional, skip_candidate) {
            push_child_edges(&snapshots[key], snapshots, &mut pending);
        }
    }
    Ok(propagation.required)
}
pub(super) struct EdgePropagation<'lock> {
    installed: HashSet<&'lock PackageKey>,
    required: HashSet<&'lock PackageKey>,
}
impl<'lock> EdgePropagation<'lock> {
    /// Whether this edge installs `key` for the first time, recording
    /// the non-optional edge that installs a skip candidate.
    fn install(
        &mut self,
        key: &'lock PackageKey,
        edge_optional: bool,
        skip_candidate: bool,
    ) -> bool {
        if skip_candidate && edge_optional {
            return false;
        }
        if skip_candidate {
            self.required.insert(key);
        }
        self.installed.insert(key)
    }
}
/// Whether the package is incompatible with the host when checked as
/// an optional dependency. A snapshot with no metadata row is not a
/// candidate; [`crate::CreateVirtualStore`] errors on it separately.
pub(super) fn is_skip_candidate(
    metadata_key: &PackageKey,
    packages: &HashMap<PackageKey, PackageMetadata>,
    base_options: &InstallabilityOptions<'_>,
    check_cache: &mut CheckCache,
) -> Result<bool, Box<InstallabilityError>> {
    let Some(metadata) = packages.get(metadata_key) else { return Ok(false) };
    Ok(cached_check(check_cache, metadata_key, metadata, true, base_options)?.is_some())
}
pub(super) fn push_child_edges<'lock>(
    snapshot: &SnapshotEntry,
    snapshots: &'lock HashMap<PackageKey, SnapshotEntry>,
    pending: &mut VecDeque<(&'lock PackageKey, bool)>,
) {
    for (target, child_edge_optional) in snapshot_edges(snapshot) {
        if let Some((child, _)) = snapshots.get_key_value(&target) {
            pending.push_back((child, child_edge_optional));
        }
    }
}
/// Iterate an importer's resolvable direct-dep edges as
/// `(snapshot key, edge is optional)` pairs. `dependencies` and
/// `devDependencies` are non-optional edges; `link:` entries resolve
/// to sibling importers, which are walk roots already, and are
/// dropped.
pub(super) fn importer_edges(
    importer: &ProjectSnapshot,
) -> impl Iterator<Item = (PackageKey, bool)> + '_ {
    let declared = importer.dependencies.iter().chain(importer.dev_dependencies.iter()).flatten();
    let required =
        declared.filter_map(|(name, spec)| spec.version.resolved_key(name)).map(|key| (key, false));
    let optional = importer
        .optional_dependencies
        .iter()
        .flatten()
        .filter_map(|(name, spec)| spec.version.resolved_key(name))
        .map(|key| (key, true));
    required.chain(optional)
}
/// Iterate a snapshot's resolvable dep edges as
/// `(snapshot key, edge is optional)` pairs.
pub(super) fn snapshot_edges(
    snapshot: &SnapshotEntry,
) -> impl Iterator<Item = (PackageKey, bool)> + '_ {
    let required = snapshot
        .dependencies
        .iter()
        .flatten()
        .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
        .map(|key| (key, false));
    let optional = snapshot
        .optional_dependencies
        .iter()
        .flatten()
        .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
        .map(|key| (key, true));
    required.chain(optional)
}
