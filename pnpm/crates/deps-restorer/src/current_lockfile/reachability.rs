use crate::platform_selection::{host_platform_selector, listed_link_platform};
use pnpm_lockfile::{
    Lockfile, PackageKey, PeerEdgeOptions, PeerSatisfactionEdges, PlatformSelector,
};
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_is_installable::SupportedArchitectures;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::Path,
    sync::Arc,
};

pub struct ReachableLockfileGraph {
    pub importer_ids: HashSet<String>,
    pub snapshot_keys: HashSet<PackageKey>,
}

/// The dependency groups a lockfile walk includes, and the peer-satisfaction
/// edges (see [`PeerSatisfactionEdges`]) it leaves out while it excludes one.
///
/// The classification is computed once per install and shared, since every
/// walk of that install over the lockfile, or over a closure of it, skips the
/// same edges. Every walk also follows the same platform variants: a variant
/// with dependencies of its own is wired as its own snapshot, and the walk
/// takes the one the install links.
#[derive(Debug, Clone)]
pub struct GroupSelection {
    pub included: IncludedDependencies,
    pub skipped_peer_edges: Arc<PeerSatisfactionEdges>,
    /// The platform whose variant a registry package links, when
    /// `supportedArchitectures` names one other than the host's.
    pub linked_platform: Option<PlatformSelector>,
    /// Which platform variants' own snapshots the walk follows.
    pub variant_walk: VariantWalk,
}

/// Which platform variants' own snapshots (`<name>@<version>/variants/<tag>`)
/// a walk follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariantWalk {
    /// Only the variant the install links, as the version's own snapshot.
    Linked,
    /// Every variant, each under its own key, as the wanted lockfile keeps
    /// them for every platform.
    Every,
}

impl GroupSelection {
    /// `included`, with `lockfile`'s peer-satisfaction edges classified when
    /// `included` leaves a group out.
    #[must_use]
    pub fn classify(
        lockfile: &Lockfile,
        included: IncludedDependencies,
        peer_edges: PeerEdgeOptions,
    ) -> Self {
        let skipped_peer_edges = if included.excludes_a_group() {
            PeerSatisfactionEdges::of_lockfile(lockfile, peer_edges)
        } else {
            PeerSatisfactionEdges::default()
        };
        GroupSelection {
            included,
            skipped_peer_edges: Arc::new(skipped_peer_edges),
            linked_platform: None,
            variant_walk: VariantWalk::Linked,
        }
    }

    /// `included` without a peer classification, for a walk that only reads
    /// which importers it reaches.
    #[must_use]
    pub fn following_every_edge(included: IncludedDependencies) -> Self {
        GroupSelection {
            included,
            skipped_peer_edges: Arc::default(),
            linked_platform: None,
            variant_walk: VariantWalk::Linked,
        }
    }

    /// This selection, walking the variants `supported` links a registry
    /// package to.
    #[must_use]
    pub fn linking_for(mut self, supported: Option<&SupportedArchitectures>) -> Self {
        self.linked_platform = listed_link_platform(supported);
        self
    }

    /// This selection, walking every variant of each version it reaches.
    #[must_use]
    pub fn walking_every_variant(mut self) -> Self {
        self.variant_walk = VariantWalk::Every;
        self
    }

    /// This selection, walking the variants `other` walks.
    #[must_use]
    pub fn linking_as(mut self, other: &GroupSelection) -> Self {
        self.linked_platform.clone_from(&other.linked_platform);
        self
    }

    /// The platform whose variants the walk follows.
    #[must_use]
    pub fn walked_platform(&self) -> PlatformSelector {
        match &self.linked_platform {
            Some(platform) => platform.clone(),
            None => host_platform_selector(),
        }
    }

    /// Each version's variants' own snapshot keys in `lockfile` the walk
    /// visits beside the version's: all of them when it walks every variant,
    /// and none when it walks the one the install links.
    fn variants_to_walk(&self, lockfile: &Lockfile) -> HashMap<PackageKey, Vec<PackageKey>> {
        match self.variant_walk {
            VariantWalk::Every => super::variant_snapshots::variant_keys_by_base(lockfile),
            VariantWalk::Linked => HashMap::new(),
        }
    }

    /// Every group, which leaves no edge out.
    #[must_use]
    pub fn all() -> Self {
        Self::following_every_edge(IncludedDependencies {
            dependencies: true,
            dev_dependencies: true,
            optional_dependencies: true,
        })
    }
}

/// The importers and snapshots `initial_importer_ids` reach through the
/// edges `groups` selects, never entering a key `should_skip` accepts.
#[must_use]
pub fn collect_reachable<ShouldSkip>(
    lockfile: &Lockfile,
    workspace_root: &Path,
    initial_importer_ids: &HashSet<String>,
    groups: &GroupSelection,
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
        groups,
        should_skip,
        variants_by_base: groups.variants_to_walk(lockfile),

        queues: WalkQueues {
            importers: initial_importer_ids
                .iter()
                .cloned()
                .collect(),
            snapshots: VecDeque::new(),
        },
    };

    while !walk.queues.importers.is_empty() || !walk.queues.snapshots.is_empty() {
        while let Some(importer_id) = walk.queues.importers.pop_front() {
            walk.visit_importer(&importer_id);
        }
        while let Some(key) = walk.queues.snapshots.pop_front() {
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
    groups: &'a GroupSelection,
    should_skip: ShouldSkip,
    /// Each version's variants' own snapshot keys, when the walk follows
    /// every variant.
    variants_by_base: HashMap<PackageKey, Vec<PackageKey>>,
    queues: WalkQueues,
}

/// The importers and snapshots a walk has yet to visit.
pub(super) struct WalkQueues {
    importers: VecDeque<String>,
    snapshots: VecDeque<PackageKey>,
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
        let included = self.groups.included;
        for map in [
            included.dependencies.then_some(importer.dependencies.as_ref()).flatten(),
            included.dev_dependencies.then_some(importer.dev_dependencies.as_ref()).flatten(),
            included
                .includes_project_optional_dependencies()
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
        let entries = self.groups.skipped_peer_edges.followed_entries(
            key,
            snapshot,
            self.groups.included.optional_dependencies,
        );
        for (alias, dep_ref) in entries {
            if let Some(target) = dep_ref.as_link_target() {
                self.enqueue_linked_importer(target);
            } else if let Some(child) = dep_ref.resolve(alias) {
                self.enqueue_snapshot(child);
            }
        }
        if let Some(variants) = self.variants_by_base.get(key).cloned() {
            for variant in variants {
                self.enqueue_snapshot(variant);
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
            self.queues.snapshots.push_back(key);
        }
    }

    fn enqueue_linked_importer(&mut self, target: &str) {
        if let Some(importer_id) =
            linked_importer_id(self.workspace_root, target, &self.known_importers)
        {
            self.queues.importers.push_back(importer_id);
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
