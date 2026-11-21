use std::collections::{
    BTreeSet,
    HashMap,
    HashSet,
};

use crate::{
    PkgName,
    PkgNameVerPeer,
    SnapshotDepRef,
    SnapshotEntry,
};

const MAX_VISITED_SNAPSHOTS: usize = 1024;
const MAX_CHILD_INSPECTIONS: usize = 65_536;
const MAX_NAME_CHECKS: usize = 262_144;

struct PeerLookup<'peer> {
    peers: &'peer [(PkgName, &'peer str)],
    by_pkg: HashMap<&'peer PkgName, usize>,
    by_name: HashMap<&'peer str, usize>,
}

impl<'peer> PeerLookup<'peer> {
    fn new(peers: &'peer [(PkgName, &'peer str)]) -> Self {
        let by_pkg = peers
            .iter()
            .enumerate()
            .map(|(idx, (peer, _))| (peer, idx))
            .collect();
        let by_name = peers
            .iter()
            .enumerate()
            .map(|(idx, (_, name))| (*name, idx))
            .collect();
        Self { peers, by_pkg, by_name }
    }
}

struct DescendantWalk<'snap, 'peer, 'links> {
    links: &'links mut LinkCandidates,
    snapshots: &'snap HashMap<PkgNameVerPeer, SnapshotEntry>,
    lookup: PeerLookup<'peer>,
    by_peer: HashMap<PkgName, Vec<PkgNameVerPeer>>,
    visited: HashMap<PkgNameVerPeer, HashSet<usize>>,
    pending: Vec<(&'snap SnapshotEntry, HashSet<usize>)>,
    inspections: usize,
    name_checks: usize,
}

pub(super) fn collect_descendant_targets<'snap, 'peer>(
    snapshots: &'snap HashMap<PkgNameVerPeer, SnapshotEntry>,
    root: &'snap SnapshotEntry,
    transitive_peers: &'peer [(PkgName, &'peer str)],
    links: &mut LinkCandidates,
) -> Option<HashMap<PkgName, Vec<PkgNameVerPeer>>> {
    let mut walk = DescendantWalk {
        links,
        snapshots,
        lookup: PeerLookup::new(transitive_peers),
        by_peer: HashMap::new(),
        visited: HashMap::new(),
        pending: vec![(root, (0..transitive_peers.len()).collect())],
        inspections: 0,
        name_checks: 0,
    };
    while let Some((current, active)) = walk.pending.pop() {
        step_descendant_children(&mut walk, current, &active)?;
    }
    Some(walk.by_peer)
}

fn step_descendant_children<'snap>(
    walk: &mut DescendantWalk<'snap, '_, '_>,
    current: &'snap SnapshotEntry,
    active: &HashSet<usize>,
) -> Option<()> {
    for (alias, dep_ref) in all_dep_refs(current) {
        walk.inspections += 1;
        if walk.inspections > MAX_CHILD_INSPECTIONS {
            return None;
        }
        let Some((child_key, child)) = resolve_child_snapshot(walk.snapshots, alias, dep_ref)
        else {
            continue;
        };
        let next = record_child_peers(walk, &child_key, child, active)?;
        if walk.visited.len() > MAX_VISITED_SNAPSHOTS {
            return None;
        }
        if !next.is_empty() {
            walk.pending.push((child, next));
        }
    }
    Some(())
}

fn resolve_child_snapshot<'a>(
    snapshots: &'a HashMap<PkgNameVerPeer, SnapshotEntry>,
    alias: &PkgName,
    dep_ref: &SnapshotDepRef,
) -> Option<(PkgNameVerPeer, &'a SnapshotEntry)> {
    let child_key = dep_ref.resolve(alias)?;
    let child = snapshots.get(&child_key)?;
    Some((child_key, child))
}

fn record_child_peers(
    walk: &mut DescendantWalk<'_, '_, '_>,
    child_key: &PkgNameVerPeer,
    child: &SnapshotEntry,
    active: &HashSet<usize>,
) -> Option<HashSet<usize>> {
    let (matched, propagated) =
        collect_child_peer_hits(&walk.lookup, &mut walk.name_checks, child, active)?;
    let mut next = HashSet::new();
    for idx in matched {
        let hit = ChildPeerHit { idx, propagates: propagated.contains(&idx) };
        if visit_child_peer(walk, child_key, child, hit) {
            next.insert(idx);
        }
    }
    Some(next)
}

fn collect_child_peer_hits(
    lookup: &PeerLookup<'_>,
    name_checks: &mut usize,
    child: &SnapshotEntry,
    active: &HashSet<usize>,
) -> Option<(BTreeSet<usize>, HashSet<usize>)> {
    let mut matched = collect_matched_dep_peers(lookup, name_checks, child, active)?;
    let propagated = collect_propagated_peers(lookup, name_checks, child, active)?;
    matched.extend(propagated.iter().copied());
    Some((matched, propagated))
}

fn collect_matched_dep_peers(
    lookup: &PeerLookup<'_>,
    name_checks: &mut usize,
    child: &SnapshotEntry,
    active: &HashSet<usize>,
) -> Option<BTreeSet<usize>> {
    let mut matched = BTreeSet::new();
    for (dep_alias, _) in all_dep_refs(child) {
        charge_name_check(name_checks)?;
        if let Some(&idx) = lookup.by_pkg
            .get(dep_alias)
            .filter(|&&idx| active.contains(&idx))
        {
            matched.insert(idx);
        }
    }
    Some(matched)
}

fn collect_propagated_peers(
    lookup: &PeerLookup<'_>,
    name_checks: &mut usize,
    child: &SnapshotEntry,
    active: &HashSet<usize>,
) -> Option<HashSet<usize>> {
    let mut propagated = HashSet::new();
    for trans_name in child.transitive_peer_dependencies.iter().flatten() {
        charge_name_check(name_checks)?;
        if let Some(&idx) = lookup.by_name
            .get(trans_name.as_str())
            .filter(|&&idx| active.contains(&idx))
        {
            propagated.insert(idx);
        }
    }
    Some(propagated)
}

fn charge_name_check(name_checks: &mut usize) -> Option<()> {
    *name_checks += 1;
    (*name_checks <= MAX_NAME_CHECKS).then_some(())
}

#[derive(Clone, Copy)]
struct ChildPeerHit {
    idx: usize,
    propagates: bool,
}

fn visit_child_peer(
    walk: &mut DescendantWalk<'_, '_, '_>,
    child_key: &PkgNameVerPeer,
    child: &SnapshotEntry,
    hit: ChildPeerHit,
) -> bool {
    if walk.visited
        .get(child_key)
        .is_some_and(|seen| seen.contains(&hit.idx))
    {
        return false;
    }
    let (peer, _) = &walk.lookup.peers[hit.idx];
    let target = resolved_peer_target(child, peer, walk.links);
    if target.is_none() && !hit.propagates {
        return false;
    }
    walk.visited
        .entry(child_key.clone())
        .or_default()
        .insert(hit.idx);
    if let Some(target) = target {
        push_unique_target(walk.by_peer.entry(peer.clone()).or_default(), target);
    }
    hit.propagates
}

pub(super) fn push_unique_target(targets: &mut Vec<PkgNameVerPeer>, target: PkgNameVerPeer) {
    if !targets.contains(&target) {
        targets.push(target);
    }
}

pub(super) enum LinkCandidates {
    Exclude { seen: bool },
    Include,
}

pub(super) fn resolved_peer_target(
    snapshot: &SnapshotEntry,
    peer: &PkgName,
    links: &mut LinkCandidates,
) -> Option<PkgNameVerPeer> {
    let reference = snapshot_dep_ref(snapshot, peer)?;
    if let LinkCandidates::Exclude { seen } = links
        && reference.as_link_target().is_some()
    {
        *seen = true;
        return None;
    }
    reference.peer_provider_key(peer)
}

fn snapshot_dep_ref<'a>(snapshot: &'a SnapshotEntry, name: &PkgName) -> Option<&'a SnapshotDepRef> {
    snapshot.dependencies
        .as_ref()
        .and_then(|deps| deps.get(name))
        .or_else(|| {
            snapshot.optional_dependencies
                .as_ref()
                .and_then(|deps| deps.get(name))
        })
}

pub(super) fn all_dep_refs(
    snapshot: &SnapshotEntry,
) -> impl Iterator<Item = (&PkgName, &SnapshotDepRef)> {
    snapshot.dependencies
        .iter()
        .chain(snapshot.optional_dependencies.iter())
        .flatten()
}
