use std::collections::HashSet;

use pnpm_crypto_hash::create_short_hash;

use super::{BoundProviders, is_patch_marker, is_peer_suffix_hash, split_peer_suffix_segments};
use crate::{
    LockfileEntries, PackageMetadata, PkgName, PkgNameVerPeer, SnapshotDepRef, SnapshotEntry,
};

pub(super) fn recover_hashed_providers(
    entries: LockfileEntries<'_>,
    key: &PkgNameVerPeer,
) -> Option<HashSet<PkgNameVerPeer>> {
    let hash_hex = suffix_hash_segment(key.suffix.peer())?;
    let snapshots = entries.snapshots?;
    let snapshot = snapshots.get(key)?;
    let package = entries.packages?.get(&key.without_peer())?;

    let own_candidates = own_peer_candidates(snapshot, package, &key.name);
    let transitive_candidates = transitive_peer_candidates(snapshots, snapshot);
    match_candidates(&own_candidates, &transitive_candidates, &hash_hex)
}

fn suffix_hash_segment(peer_suffix: &str) -> Option<String> {
    let segments = split_peer_suffix_segments(peer_suffix)?;
    let mut provider_segments = segments
        .into_iter()
        .filter(|segment| !is_patch_marker(segment));
    let only = provider_segments.next()?;
    (provider_segments.next().is_none() && is_peer_suffix_hash(&only)).then_some(only)
}

fn own_peer_candidates(
    snapshot: &SnapshotEntry,
    package: &PackageMetadata,
    self_name: &PkgName,
) -> Vec<PkgNameVerPeer> {
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
        .collect();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .filter_map(|name| name.parse::<PkgName>().ok())
        .filter(|name| name != self_name)
        .filter_map(|name| snapshot_dep_ref(snapshot, &name)?.resolve(&name))
        .collect()
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

fn transitive_peer_candidates(
    snapshots: &std::collections::HashMap<PkgNameVerPeer, SnapshotEntry>,
    snapshot: &SnapshotEntry,
) -> Vec<Vec<PkgNameVerPeer>> {
    snapshot.transitive_peer_dependencies
        .iter()
        .flatten()
        .filter_map(|name| {
            let peer = name.parse::<PkgName>().ok()?;
            let targets = descendant_peer_targets(snapshots, snapshot, &peer, name);
            (!targets.is_empty()).then_some(targets)
        })
        .collect()
}

fn descendant_peer_targets(
    snapshots: &std::collections::HashMap<PkgNameVerPeer, SnapshotEntry>,
    root: &SnapshotEntry,
    peer: &PkgName,
    peer_name: &str,
) -> Vec<PkgNameVerPeer> {
    let mut targets = Vec::new();
    let mut visited = HashSet::new();
    let mut pending = vec![root];
    while let Some(current) = pending.pop() {
        for child in unvisited_child_snapshots(snapshots, current, &mut visited) {
            record_peer_target(&mut targets, child, peer);
            if lists_transitive_peer(child, peer_name) {
                pending.push(child);
            }
        }
    }
    targets
}

fn record_peer_target(targets: &mut Vec<PkgNameVerPeer>, snapshot: &SnapshotEntry, peer: &PkgName) {
    if let Some(target) = resolved_peer_target(snapshot, peer)
        && !targets.contains(&target)
    {
        targets.push(target);
    }
}

fn unvisited_child_snapshots<'a>(
    snapshots: &'a std::collections::HashMap<PkgNameVerPeer, SnapshotEntry>,
    parent: &'a SnapshotEntry,
    visited: &mut HashSet<PkgNameVerPeer>,
) -> impl Iterator<Item = &'a SnapshotEntry> {
    all_dep_refs(parent)
        .filter_map(move |(alias, dep_ref)| {
            let child_key = dep_ref.resolve(alias)?;
            visited
                .insert(child_key.clone())
                .then(|| snapshots.get(&child_key))
                .flatten()
        })
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
    own: &[PkgNameVerPeer],
    transitive: &[Vec<PkgNameVerPeer>],
    hash_hex: &str,
) -> Option<HashSet<PkgNameVerPeer>> {
    if own.len() > 20
        || transitive
            .iter()
            .map(Vec::len)
            .product::<usize>()
            > 64
    {
        return None;
    }
    let mut chosen_transitive = Vec::with_capacity(transitive.len());
    find_matching_combination(own, transitive, 0, &mut chosen_transitive, hash_hex)
}

fn find_matching_combination<'a>(
    own: &'a [PkgNameVerPeer],
    transitive: &'a [Vec<PkgNameVerPeer>],
    trans_idx: usize,
    chosen_transitive: &mut Vec<&'a PkgNameVerPeer>,
    hash_hex: &str,
) -> Option<HashSet<PkgNameVerPeer>> {
    if trans_idx < transitive.len() {
        for candidate in &transitive[trans_idx] {
            chosen_transitive.push(candidate);
            if let Some(matched) = find_matching_combination(
                own,
                transitive,
                trans_idx + 1,
                chosen_transitive,
                hash_hex,
            ) {
                return Some(matched);
            }
            chosen_transitive.pop();
        }
        return None;
    }
    match_own_subsets(own, chosen_transitive, hash_hex)
}

fn match_own_subsets(
    own: &[PkgNameVerPeer],
    chosen_transitive: &[&PkgNameVerPeer],
    hash_hex: &str,
) -> Option<HashSet<PkgNameVerPeer>> {
    let full_mask = (1_u32 << own.len()) - 1;
    for strip_peers in [false, true] {
        for mask in (0..=full_mask).rev() {
            if subset_matches_hash(own, chosen_transitive, mask, strip_peers, hash_hex) {
                return Some(collect_providers(own, chosen_transitive, mask));
            }
        }
    }
    None
}

fn subset_matches_hash(
    own: &[PkgNameVerPeer],
    chosen_transitive: &[&PkgNameVerPeer],
    mask: u32,
    strip_peers: bool,
    hash_hex: &str,
) -> bool {
    let mut segments: Vec<String> = own
        .iter()
        .enumerate()
        .filter(|(idx, _)| (mask & (1_u32 << idx)) != 0)
        .map(|(_, target)| render_segment(target, strip_peers))
        .chain(chosen_transitive.iter().map(|target| render_segment(target, strip_peers)))
        .collect();
    if segments.is_empty() {
        return false;
    }
    segments.sort();
    create_short_hash(&segments.join(")(")) == hash_hex
}

fn render_segment(target: &PkgNameVerPeer, strip_peers: bool) -> String {
    if strip_peers { target.without_peer().to_string() } else { target.to_string() }
}

fn collect_providers(
    own: &[PkgNameVerPeer],
    chosen_transitive: &[&PkgNameVerPeer],
    mask: u32,
) -> HashSet<PkgNameVerPeer> {
    own.iter()
        .enumerate()
        .filter(|(idx, _)| (mask & (1_u32 << idx)) != 0)
        .map(|(_, target)| target.without_peer())
        .chain(chosen_transitive.iter().map(|target| target.without_peer()))
        .collect()
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
