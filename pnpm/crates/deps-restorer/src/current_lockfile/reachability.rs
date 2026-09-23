use pnpm_lockfile::{Lockfile, PackageKey, PeerSatisfactionEdges};
use pnpm_modules_yaml::IncludedDependencies;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::Path,
};

pub struct ReachableLockfileGraph {
    pub importer_ids: HashSet<String>,
    pub snapshot_keys: HashSet<PackageKey>,
}

/// The edges a walk follows: the snapshot entries of the included groups,
/// minus the peer-satisfaction edges while a group is left out.
pub(super) struct WalkedEdges {
    pub(super) included: IncludedDependencies,
    pub(super) skipped_peer_edges: PeerSatisfactionEdges,
}

impl WalkedEdges {
    pub(super) fn of(lockfile: &Lockfile, groups: super::GroupSelection) -> Self {
        let included = groups.included;
        let skipped_peer_edges = if groups.included.excludes_a_group() {
            PeerSatisfactionEdges::of_lockfile(lockfile, groups.peer_edges)
        } else {
            PeerSatisfactionEdges::default()
        };
        WalkedEdges { included, skipped_peer_edges }
    }
}

/// The importers and snapshots `initial_importer_ids` reach through the
/// edges `groups` selects, never entering a key `should_skip` accepts.
#[must_use]
pub fn collect_reachable<ShouldSkip>(
    lockfile: &Lockfile,
    workspace_root: &Path,
    initial_importer_ids: &HashSet<String>,
    groups: super::GroupSelection,
    should_skip: ShouldSkip,
) -> ReachableLockfileGraph
where
    ShouldSkip: Fn(&PackageKey) -> bool,
{
    let edges = WalkedEdges::of(lockfile, groups);
    walk_reachable(lockfile, workspace_root, initial_importer_ids, &edges, should_skip)
}

pub(super) fn walk_reachable<ShouldSkip>(
    lockfile: &Lockfile,
    workspace_root: &Path,
    initial_importer_ids: &HashSet<String>,
    edges: &WalkedEdges,
    should_skip: ShouldSkip,
) -> ReachableLockfileGraph
where
    ShouldSkip: Fn(&PackageKey) -> bool,
{
    let mut known_importer_ids = lockfile.importers
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    known_importer_ids.sort();
    let mut walk = ReachableWalk {
        reached: ReachableLockfileGraph {
            importer_ids: HashSet::new(),
            snapshot_keys: HashSet::new(),
        },
        lockfile,
        workspace_root,
        known_importers: known_importer_ids
            .into_iter()
            .map(|id| {
                (pnpm_fs::lexical_normalize(&crate::importer_root_dir(workspace_root, &id)), id)
            })
            .collect(),
        edges,
        should_skip,

        importer_queue: initial_importer_ids
            .iter()
            .cloned()
            .collect(),
        snapshot_queue: VecDeque::new(),
    };

    while !walk.importer_queue.is_empty() || !walk.snapshot_queue.is_empty() {
        while let Some(importer_id) = walk.importer_queue.pop_front() {
            walk.visit_importer(&importer_id);
        }
        while let Some(key) = walk.snapshot_queue.pop_front() {
            walk.visit_snapshot(&key);
        }
    }

    walk.reached
}
/// Breadth-first walk of the lockfile graph from a set of importers.
/// Importers and snapshots reach each other in both directions —
/// a snapshot's `link:` dependency pulls in a workspace importer — so
/// the two queues run to exhaustion in turn until neither grows.
pub(super) struct ReachableWalk<'a, ShouldSkip> {
    pub reached: ReachableLockfileGraph,
    lockfile: &'a Lockfile,
    workspace_root: &'a Path,
    known_importers: HashMap<std::path::PathBuf, String>,
    edges: &'a WalkedEdges,
    should_skip: ShouldSkip,
    importer_queue: VecDeque<String>,
    snapshot_queue: VecDeque<PackageKey>,
}
impl<ShouldSkip: Fn(&PackageKey) -> bool> ReachableWalk<'_, ShouldSkip> {
    fn visit_importer(&mut self, importer_id: &str) {
        if self.reached.importer_ids.contains(importer_id) {
            return;
        }
        let Some(importer) = self.lockfile.importers.get(importer_id) else {
            return;
        };
        self.reached.importer_ids.insert(importer_id.to_owned());
        let included = self.edges.included;
        for map in [
            included.dependencies.then_some(importer.dependencies.as_ref()).flatten(),
            included.dev_dependencies.then_some(importer.dev_dependencies.as_ref()).flatten(),
            included.optional_dependencies
                .then_some(importer.optional_dependencies.as_ref())
                .flatten(),
        ]
        .into_iter()
        .flatten()
        {
            for (name, spec) in map {
                if let Some(key) = spec.version.resolved_key(name) {
                    self.enqueue_snapshot(key);
                }
            }
        }
    }

    fn visit_snapshot(&mut self, key: &PackageKey) {
        if !self.reached.snapshot_keys.insert(key.clone()) {
            return;
        }
        let Some(snapshot) = self.lockfile.snapshots
            .as_ref()
            .and_then(|snapshots| snapshots.get(key))
        else {
            return;
        };
        let entries = self.edges.skipped_peer_edges.followed_entries(
            key,
            snapshot,
            self.edges.included.optional_dependencies,
        );
        for (alias, dep_ref) in entries {
            if let Some(target) = dep_ref.as_link_target() {
                self.enqueue_linked_importer(target);
            } else if let Some(child) = dep_ref.resolve(alias) {
                self.enqueue_snapshot(child);
            }
        }
    }

    /// A key the caller skips, or one the lockfile has no snapshot for,
    /// reaches nothing: neither it nor its subtree is materialized.
    fn enqueue_snapshot(&mut self, key: PackageKey) {
        if (self.should_skip)(&key) {
            return;
        }
        if self.lockfile.snapshots
            .as_ref()
            .is_some_and(|snapshots| snapshots.contains_key(&key))
        {
            self.snapshot_queue.push_back(key);
        }
    }

    fn enqueue_linked_importer(&mut self, target: &str) {
        if let Some(importer_id) =
            linked_importer_id(self.workspace_root, target, &self.known_importers)
        {
            self.importer_queue.push_back(importer_id);
        }
    }
}
pub(super) fn linked_importer_id(
    base: &Path,
    target: &str,
    known_importers: &HashMap<std::path::PathBuf, String>,
) -> Option<String> {
    let target = Path::new(target);
    let resolved = if target.is_absolute() {
        pnpm_fs::lexical_normalize(target)
    } else {
        pnpm_fs::lexical_normalize(&base.join(target))
    };
    known_importers.get(&resolved).cloned()
}
