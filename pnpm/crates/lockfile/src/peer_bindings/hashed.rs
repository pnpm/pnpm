use std::{borrow::Cow, collections::HashSet};

use pnpm_crypto_hash::create_short_hash;

use super::{BoundProviders, is_patch_marker, is_peer_suffix_hash, split_peer_suffix_segments};
use crate::{
    LockfileEntries, PackageMetadata, PkgName, PkgNameVerPeer, SnapshotDepRef, SnapshotEntry,
};

const MAX_COMBINATIONS: usize = 4096;
const MAX_VISITED_SNAPSHOTS: usize = 1024;

pub(super) fn recover_hashed_providers(
    entries: LockfileEntries<'_>,
    key: &PkgNameVerPeer,
) -> Option<HashSet<PkgNameVerPeer>> {
    let hash_hex = suffix_hash_segment(key.suffix.peer())?;
    let snapshots = entries.snapshots?;
    let snapshot = snapshots.get(key)?;
    let package = entries.packages?.get(&key.without_peer())?;

    let candidates = peer_candidates_by_name(snapshots, snapshot, package, key)?;
    match_candidates(&candidates, &hash_hex)
}

fn suffix_hash_segment(peer_suffix: &str) -> Option<String> {
    let segments = split_peer_suffix_segments(peer_suffix)?;
    let mut provider_segments = segments
        .into_iter()
        .filter(|segment| !is_patch_marker(segment));
    let only = provider_segments.next()?;
    (provider_segments.next().is_none() && is_peer_suffix_hash(&only)).then_some(only)
}

fn peer_candidates_by_name(
    snapshots: &std::collections::HashMap<PkgNameVerPeer, SnapshotEntry>,
    snapshot: &SnapshotEntry,
    package: &PackageMetadata,
    self_key: &PkgNameVerPeer,
) -> Option<Vec<Vec<PkgNameVerPeer>>> {
    let peers = peer_names_of_snapshot(snapshot, package);
    let self_identity = self_key.without_peer();
    let transitive_peers: Vec<(PkgName, &str)> = peers
        .iter()
        .filter(|(_, name)| lists_transitive_peer(snapshot, name))
        .cloned()
        .collect();
    let descendant_targets = if transitive_peers.is_empty() {
        std::collections::HashMap::new()
    } else {
        collect_descendant_targets(snapshots, snapshot, &transitive_peers)?
    };
    Some(
        peers
            .into_iter()
            .map(|(peer, _)| {
                let mut targets = targets_for_peer(
                    snapshot,
                    &peer,
                    descendant_targets.get(&peer).map(Vec::as_slice),
                );
                if peer == self_key.name {
                    targets.retain(|target| target.without_peer() != self_identity);
                }
                targets
            })
            .filter(|targets| !targets.is_empty())
            .collect(),
    )
}

fn peer_names_of_snapshot<'a>(
    snapshot: &'a SnapshotEntry,
    package: &'a PackageMetadata,
) -> Vec<(PkgName, &'a str)> {
    let mut names: Vec<&str> = package.peer_dependencies
        .iter()
        .flatten()
        .map(|(name, _)| name.as_str())
        .chain(
            package.peer_dependencies_meta
                .iter()
                .flatten()
                .map(|(name, _)| name.as_str()),
        )
        .chain(
            snapshot.transitive_peer_dependencies
                .iter()
                .flatten()
                .map(String::as_str),
        )
        .collect();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .filter_map(|name| {
            let peer = name.parse::<PkgName>().ok()?;
            Some((peer, name))
        })
        .collect()
}

fn targets_for_peer(
    snapshot: &SnapshotEntry,
    peer: &PkgName,
    descendants: Option<&[PkgNameVerPeer]>,
) -> Vec<PkgNameVerPeer> {
    let mut targets = Vec::new();
    record_peer_target(&mut targets, snapshot, peer);
    for target in descendants.into_iter().flatten() {
        if !targets.contains(target) {
            targets.push(target.clone());
        }
    }
    targets
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

fn collect_descendant_targets(
    snapshots: &std::collections::HashMap<PkgNameVerPeer, SnapshotEntry>,
    root: &SnapshotEntry,
    transitive_peers: &[(PkgName, &str)],
) -> Option<std::collections::HashMap<PkgName, Vec<PkgNameVerPeer>>> {
    let mut by_peer: std::collections::HashMap<PkgName, Vec<PkgNameVerPeer>> =
        std::collections::HashMap::new();
    let mut visited: std::collections::HashMap<PkgNameVerPeer, HashSet<usize>> =
        std::collections::HashMap::new();
    let initial: Vec<usize> = (0..transitive_peers.len()).collect();
    let mut pending = vec![(root, initial)];
    while let Some((current, active)) = pending.pop() {
        for (child_key, child) in resolved_children(snapshots, current) {
            let next = record_child_peers(
                &mut by_peer,
                &mut visited,
                &child_key,
                child,
                &active,
                transitive_peers,
            );
            if visited.len() > MAX_VISITED_SNAPSHOTS {
                return None;
            }
            if !next.is_empty() {
                pending.push((child, next));
            }
        }
    }
    Some(by_peer)
}

fn record_child_peers(
    by_peer: &mut std::collections::HashMap<PkgName, Vec<PkgNameVerPeer>>,
    visited: &mut std::collections::HashMap<PkgNameVerPeer, HashSet<usize>>,
    child_key: &PkgNameVerPeer,
    child: &SnapshotEntry,
    active: &[usize],
    transitive_peers: &[(PkgName, &str)],
) -> Vec<usize> {
    let seen = visited.entry(child_key.clone()).or_default();
    let mut next = Vec::new();
    for &idx in active {
        if !seen.insert(idx) {
            continue;
        }
        let (peer, peer_name) = &transitive_peers[idx];
        record_peer_target(by_peer.entry(peer.clone()).or_default(), child, peer);
        if lists_transitive_peer(child, peer_name) {
            next.push(idx);
        }
    }
    next
}

fn resolved_children<'a>(
    snapshots: &'a std::collections::HashMap<PkgNameVerPeer, SnapshotEntry>,
    parent: &'a SnapshotEntry,
) -> impl Iterator<Item = (PkgNameVerPeer, &'a SnapshotEntry)> {
    all_dep_refs(parent)
        .filter_map(move |(alias, dep_ref)| {
            let child_key = dep_ref.resolve(alias)?;
            let child = snapshots.get(&child_key)?;
            Some((child_key, child))
        })
}

fn record_peer_target(targets: &mut Vec<PkgNameVerPeer>, snapshot: &SnapshotEntry, peer: &PkgName) {
    if let Some(target) = resolved_peer_target(snapshot, peer)
        && !targets.contains(&target)
    {
        targets.push(target);
    }
}

fn resolved_peer_target(snapshot: &SnapshotEntry, peer: &PkgName) -> Option<PkgNameVerPeer> {
    snapshot_dep_ref(snapshot, peer)?.resolve(peer)
}

fn all_dep_refs(snapshot: &SnapshotEntry) -> impl Iterator<Item = (&PkgName, &SnapshotDepRef)> {
    snapshot.dependencies
        .iter()
        .chain(snapshot.optional_dependencies.iter())
        .flatten()
}

fn lists_transitive_peer(snapshot: &SnapshotEntry, peer_name: &str) -> bool {
    snapshot.transitive_peer_dependencies
        .iter()
        .flatten()
        .any(|name| name == peer_name)
}

fn match_candidates(
    candidates: &[Vec<PkgNameVerPeer>],
    hash_hex: &str,
) -> Option<HashSet<PkgNameVerPeer>> {
    for mode in [PeerGroups::Full, PeerGroups::Bare] {
        if let Some(matched) = CombinationSearch::new(mode, hash_hex).find(candidates) {
            return Some(matched);
        }
    }
    if !candidates
        .iter()
        .flatten()
        .any(|target| !target.suffix.peer().is_empty())
    {
        return None;
    }
    // Cycle cuts can omit nested groups from only some providers.
    let mixed = mixed_cycle_cut_candidates(candidates);
    if mixed.as_slice() == candidates {
        return None;
    }
    CombinationSearch::new(PeerGroups::Full, hash_hex).find(&mixed)
}

fn mixed_cycle_cut_candidates(candidates: &[Vec<PkgNameVerPeer>]) -> Vec<Vec<PkgNameVerPeer>> {
    candidates
        .iter()
        .map(|targets| {
            let mut variants = targets.clone();
            let mut seen: HashSet<_> = targets.iter().cloned().collect();
            for target in targets {
                let bare = target.without_peer();
                if seen.insert(bare.clone()) {
                    variants.push(bare);
                }
            }
            variants
        })
        .collect()
}

#[derive(Clone, Copy)]
enum PeerGroups {
    Full,
    Bare,
}

impl PeerGroups {
    fn provider(self, target: &PkgNameVerPeer) -> Cow<'_, PkgNameVerPeer> {
        match self {
            Self::Full => Cow::Borrowed(target),
            Self::Bare => Cow::Owned(target.without_peer()),
        }
    }
}

struct CombinationSearch<'hash> {
    mode: PeerGroups,
    hash_hex: &'hash str,
    remaining: usize,
}

impl<'hash> CombinationSearch<'hash> {
    fn new(mode: PeerGroups, hash_hex: &'hash str) -> Self {
        CombinationSearch { mode, hash_hex, remaining: MAX_COMBINATIONS }
    }

    fn find(&mut self, candidates: &[Vec<PkgNameVerPeer>]) -> Option<HashSet<PkgNameVerPeer>> {
        let mut chosen = Vec::with_capacity(candidates.len());
        // Prefer complete provider sets before spending the budget on omissions.
        for omitted in 0..=candidates.len() {
            if self.remaining == 0 {
                break;
            }
            if let Some(matched) = self.with_omissions(candidates, &mut chosen, omitted) {
                return Some(matched);
            }
        }
        None
    }

    fn with_omissions<'a>(
        &mut self,
        candidates: &'a [Vec<PkgNameVerPeer>],
        chosen: &mut Vec<&'a PkgNameVerPeer>,
        omitted: usize,
    ) -> Option<HashSet<PkgNameVerPeer>> {
        let mut frames = vec![SearchFrame { next_target: 0, omitted, chosen_len: 0 }];
        while self.remaining > 0 && !frames.is_empty() {
            let depth = frames.len() - 1;
            let frame = frames.last_mut().unwrap();
            chosen.truncate(frame.chosen_len);
            if frame.omitted > candidates.len() - depth {
                frames.pop();
            } else if depth == candidates.len() {
                frames.pop();
                if let Some(matched) = self.match_chosen(chosen) {
                    return Some(matched);
                }
            } else if let Some(next) = frame.next(&candidates[depth], chosen) {
                frames.push(next);
            } else {
                frames.pop();
            }
        }
        None
    }

    fn match_chosen(&mut self, chosen: &[&PkgNameVerPeer]) -> Option<HashSet<PkgNameVerPeer>> {
        self.remaining -= 1;
        subset_matches_hash(chosen, self.mode, self.hash_hex)
            .then(|| {
                chosen
                    .iter()
                    .map(|target| self.mode.provider(target).into_owned())
                    .collect()
            })
    }
}

struct SearchFrame {
    next_target: usize,
    omitted: usize,
    chosen_len: usize,
}

impl SearchFrame {
    fn next<'a>(
        &mut self,
        targets: &'a [PkgNameVerPeer],
        chosen: &mut Vec<&'a PkgNameVerPeer>,
    ) -> Option<Self> {
        let omitted = if let Some(target) = targets.get(self.next_target) {
            chosen.push(target);
            self.omitted
        } else if self.next_target == targets.len() && self.omitted > 0 {
            self.omitted - 1
        } else {
            return None;
        };
        self.next_target += 1;
        Some(Self { next_target: 0, omitted, chosen_len: chosen.len() })
    }
}

fn subset_matches_hash(chosen: &[&PkgNameVerPeer], mode: PeerGroups, hash_hex: &str) -> bool {
    if chosen.is_empty() {
        return false;
    }
    let mut segments: Vec<String> = chosen
        .iter()
        .map(|target| mode.provider(target).to_string())
        .collect();
    segments.sort();
    create_short_hash(&segments.join(")(")) == hash_hex
}

impl BoundProviders {
    /// The providers recorded for `key`, falling back to its suffix and hashed
    /// recovery when the snapshot has no recorded bindings for that key.
    #[must_use]
    pub fn of_snapshot(entries: LockfileEntries<'_>, key: &PkgNameVerPeer) -> Self {
        let providers = Self::from_key(key);
        if !providers.is_hashed() {
            return providers;
        }
        if let Some(providers) = entries.snapshots
            .and_then(|snapshots| snapshots.get(key))
            .and_then(|snapshot| snapshot.recorded_peer_bindings.as_ref())
            .and_then(|bindings| bindings.providers_for(key))
        {
            return providers.clone();
        }
        match recover_hashed_providers(entries, key) {
            Some(recovered) => Self::from_providers(recovered),
            None => providers,
        }
    }
}

#[cfg(test)]
mod tests;
