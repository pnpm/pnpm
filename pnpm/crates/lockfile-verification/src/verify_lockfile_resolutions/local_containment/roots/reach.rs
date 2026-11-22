use std::collections::{
    HashMap,
    HashSet,
};

use pnpm_lockfile::{
    Lockfile,
    PkgNameVerPeer,
    SnapshotDepRef,
    SnapshotEntry,
    package_root_link_target,
};

use super::super::fast_scan::package_has_local_surface;

/// The snapshot graph of a lockfile with integer node ids, so a workspace
/// with many importers whose closures overlap can be walked once per importer
/// without hashing snapshot keys.
///
/// A node is any snapshot key, child key, or importer seed, even one missing
/// from `snapshots`. A node is *relevant* when local containment can act on
/// it: its package has a local surface, or its snapshot links to a host path
/// or depends on a package with a local surface. Every other node passes the
/// containment checks whichever importer reaches it.
pub(super) struct SnapshotGraph {
    keys: Vec<PkgNameVerPeer>,
    /// Keys come from the lockfile, so the map keeps the randomly seeded
    /// default hasher.
    ids: HashMap<PkgNameVerPeer, u32>,
    children: Vec<Vec<u32>>,
    relevant: Vec<bool>,
    leads_to_relevant: Vec<bool>,
    /// The children of each node that lead to a relevant node.
    pruned_children: Vec<Vec<u32>>,
}

/// What a set of importers reaches, as keys.
#[derive(Default)]
pub(in super::super) struct Reached {
    pub(in super::super) snapshots: HashSet<PkgNameVerPeer>,
    pub(in super::super) packages: HashSet<PkgNameVerPeer>,
}

impl SnapshotGraph {
    pub(super) fn build<'s>(
        lockfile: &Lockfile,
        seeds: impl IntoIterator<Item = &'s PkgNameVerPeer>,
    ) -> Self {
        let mut graph = Self {
            keys: Vec::new(),
            ids: HashMap::new(),
            children: Vec::new(),
            relevant: Vec::new(),
            leads_to_relevant: Vec::new(),
            pruned_children: Vec::new(),
        };
        let mut links_host_path = Vec::new();
        for (snap_key, snapshot) in lockfile.snapshots.iter().flatten() {
            let id = graph.intern(snap_key);
            if graph.add_children(id, snapshot) {
                links_host_path.push(id);
            }
        }
        for seed in seeds {
            graph.intern(seed);
        }
        graph.mark_relevant(lockfile, &links_host_path);
        graph.mark_leads_to_relevant();
        graph.prune_children();
        graph
    }

    /// Relevant nodes the `seeds` reach, walking only nodes that lead to one.
    /// `stamps` is shared across calls so each walk allocates nothing per node.
    pub(super) fn reach_relevant(&self, seeds: &[PkgNameVerPeer], stamps: &mut Stamps) -> Reached {
        let stamp = stamps.next(self.keys.len());
        let mut reached = Reached::default();
        let mut stack: Vec<u32> = self
            .seed_ids(seeds)
            .filter(|&id| self.leads_to_relevant[id as usize])
            .collect();
        while let Some(id) = stack.pop() {
            let idx = id as usize;
            if !stamps.visit(idx, stamp) {
                continue;
            }
            if self.relevant[idx] {
                self.record(idx, &mut reached);
            }
            stack.extend(self.pruned_children[idx].iter().copied());
        }
        reached
    }

    /// Every node the `seeds` reach.
    pub(super) fn reach_all<'s>(
        &self,
        seeds: impl IntoIterator<Item = &'s PkgNameVerPeer>,
    ) -> Reached {
        let mut visited = vec![false; self.keys.len()];
        let mut reached = Reached::default();
        let mut stack: Vec<u32> = seeds
            .into_iter()
            .filter_map(|seed| self.ids.get(seed).copied())
            .collect();
        while let Some(id) = stack.pop() {
            let idx = id as usize;
            if std::mem::replace(&mut visited[idx], true) {
                continue;
            }
            self.record(idx, &mut reached);
            stack.extend(self.children[idx].iter().copied());
        }
        reached
    }

    fn seed_ids<'g>(&'g self, seeds: &'g [PkgNameVerPeer]) -> impl Iterator<Item = u32> + 'g {
        seeds
            .iter()
            .filter_map(|seed| self.ids.get(seed).copied())
    }

    fn record(&self, idx: usize, reached: &mut Reached) {
        let key = &self.keys[idx];
        reached.packages.insert(key.without_peer());
        reached.snapshots.insert(key.clone());
    }

    fn intern(&mut self, key: &PkgNameVerPeer) -> u32 {
        if let Some(&id) = self.ids.get(key) {
            return id;
        }
        let id = u32::try_from(self.keys.len()).expect("snapshot count fits in u32");
        self.keys.push(key.clone());
        self.ids.insert(key.clone(), id);
        self.children.push(Vec::new());
        id
    }

    /// Add the resolved children of `snapshot` to node `id`. Returns whether it
    /// links to a path outside every package root.
    fn add_children(&mut self, id: u32, snapshot: &SnapshotEntry) -> bool {
        let mut links_host_path = false;
        let all_deps = snapshot.dependencies
            .iter()
            .flatten()
            .chain(snapshot.optional_dependencies.iter().flatten());
        for (alias, dep_ref) in all_deps {
            if let SnapshotDepRef::Link(target) = dep_ref {
                links_host_path |= package_root_link_target(target).is_none();
            } else if let Some(child_key) = dep_ref.resolve(alias) {
                let child = self.intern(&child_key);
                self.children[id as usize].push(child);
            }
        }
        links_host_path
    }

    fn mark_relevant(&mut self, lockfile: &Lockfile, links_host_path: &[u32]) {
        let packages = lockfile.packages.as_ref();
        let local_package: Vec<bool> = self.keys
            .iter()
            .map(|key| {
                let base_key = key.without_peer();
                package_has_local_surface(&base_key, packages.and_then(|pkgs| pkgs.get(&base_key)))
            })
            .collect();
        self.relevant = (0..self.keys.len())
            .map(|idx| {
                local_package[idx]
                    || self.children[idx]
                        .iter()
                        .any(|&child| local_package[child as usize])
            })
            .collect();
        for &id in links_host_path {
            self.relevant[id as usize] = true;
        }
    }

    fn mark_leads_to_relevant(&mut self) {
        let mut parents: Vec<Vec<u32>> = vec![Vec::new(); self.keys.len()];
        for (parent, children) in self.children.iter().enumerate() {
            let parent = u32::try_from(parent).expect("snapshot count fits in u32");
            for &child in children {
                parents[child as usize].push(parent);
            }
        }
        self.leads_to_relevant = self.relevant.clone();
        let mut stack: Vec<usize> = (0..self.keys.len())
            .filter(|&idx| self.relevant[idx])
            .collect();
        while let Some(idx) = stack.pop() {
            for &parent in &parents[idx] {
                let parent = parent as usize;
                if !std::mem::replace(&mut self.leads_to_relevant[parent], true) {
                    stack.push(parent);
                }
            }
        }
    }

    fn prune_children(&mut self) {
        self.pruned_children = self.children
            .iter()
            .map(|children| {
                children
                    .iter()
                    .copied()
                    .filter(|&child| self.leads_to_relevant[child as usize])
                    .collect()
            })
            .collect();
    }
}

/// Per-node visit stamps reused across walks: a node is visited in a walk when
/// its stamp equals that walk's stamp.
#[derive(Default)]
pub(super) struct Stamps {
    stamps: Vec<u32>,
    current: u32,
}

impl Stamps {
    fn next(&mut self, node_count: usize) -> u32 {
        self.stamps.resize(node_count, 0);
        self.current += 1;
        self.current
    }

    fn visit(&mut self, idx: usize, stamp: u32) -> bool {
        std::mem::replace(&mut self.stamps[idx], stamp) != stamp
    }
}
