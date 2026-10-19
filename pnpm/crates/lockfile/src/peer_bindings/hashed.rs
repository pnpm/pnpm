use std::collections::HashSet;

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

    let candidates = peer_candidates_by_name(snapshots, snapshot, package, &key.name)?;
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
    self_name: &PkgName,
) -> Option<Vec<Vec<PkgNameVerPeer>>> {
    let peers = peer_names_of_snapshot(snapshot, package, self_name);
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
                targets_for_peer(snapshot, &peer, descendant_targets.get(&peer).map(Vec::as_slice))
            })
            .filter(|targets| !targets.is_empty())
            .collect(),
    )
}

fn peer_names_of_snapshot<'a>(
    snapshot: &'a SnapshotEntry,
    package: &'a PackageMetadata,
    self_name: &PkgName,
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
            (peer != *self_name).then_some((peer, name))
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
    let total = candidates
        .iter()
        .try_fold(1_usize, |acc, targets| acc.checked_mul(targets.len() + 1))?;
    if total > MAX_COMBINATIONS {
        return None;
    }
    let mut chosen = Vec::with_capacity(candidates.len());
    for strip_peers in [false, true] {
        if let Some(matched) =
            find_matching_combination(candidates, 0, &mut chosen, strip_peers, hash_hex)
        {
            return Some(matched);
        }
    }
    None
}

fn find_matching_combination<'a>(
    candidates: &'a [Vec<PkgNameVerPeer>],
    idx: usize,
    chosen: &mut Vec<&'a PkgNameVerPeer>,
    strip_peers: bool,
    hash_hex: &str,
) -> Option<HashSet<PkgNameVerPeer>> {
    if idx < candidates.len() {
        for candidate in &candidates[idx] {
            chosen.push(candidate);
            if let Some(matched) =
                find_matching_combination(candidates, idx + 1, chosen, strip_peers, hash_hex)
            {
                return Some(matched);
            }
            chosen.pop();
        }
        return find_matching_combination(candidates, idx + 1, chosen, strip_peers, hash_hex);
    }
    subset_matches_hash(chosen, strip_peers, hash_hex)
        .then(|| {
            chosen
                .iter()
                .map(|target| target.without_peer())
                .collect()
        })
}

fn subset_matches_hash(chosen: &[&PkgNameVerPeer], strip_peers: bool, hash_hex: &str) -> bool {
    if chosen.is_empty() {
        return false;
    }
    let mut segments: Vec<String> = chosen
        .iter()
        .map(|target| render_segment(target, strip_peers))
        .collect();
    segments.sort();
    create_short_hash(&segments.join(")(")) == hash_hex
}

fn render_segment(target: &PkgNameVerPeer, strip_peers: bool) -> String {
    if strip_peers { target.without_peer().to_string() } else { target.to_string() }
}

impl BoundProviders {
    /// The providers `key`'s peer suffix names, recovering them from `entries`
    /// when `key` carries a hashed suffix.
    #[must_use]
    pub fn of_snapshot(entries: LockfileEntries<'_>, key: &PkgNameVerPeer) -> Self {
        let providers = Self::from_key(key);
        if !providers.is_hashed() {
            return providers;
        }
        match recover_hashed_providers(entries, key) {
            Some(recovered) => BoundProviders(Some(recovered)),
            None => providers,
        }
    }
}
