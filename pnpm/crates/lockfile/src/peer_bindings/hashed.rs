use std::collections::{
    HashMap,
    HashSet,
};

use super::{
    BoundProviders,
    is_patch_marker,
    is_peer_suffix_hash,
    split_peer_suffix_segments,
};
use crate::{
    LockfileEntries,
    PackageMetadata,
    PkgName,
    PkgNameVerPeer,
    SnapshotEntry,
};
use cycle_probe::CycleProbe;
use descendants::{
    all_dep_refs,
    collect_descendant_targets,
    push_unique_target,
    resolved_peer_target,
};
use search::{
    CombinationSearch,
    MixedCandidate,
    PeerGroups,
    pair_segment_of,
};

mod cycle_probe;
mod descendants;
mod search;

pub(super) fn recover_hashed_providers(
    entries: LockfileEntries<'_>,
    key: &PkgNameVerPeer,
) -> Option<HashSet<PkgNameVerPeer>> {
    let hash_hex = suffix_hash_segment(key.suffix.peer())?;
    let snapshots = entries.snapshots?;
    let snapshot = snapshots.get(key)?;
    let packages = entries.packages?;
    let package = packages.get(&key.without_peer())?;

    let candidates = peer_candidates_by_name(snapshots, snapshot, package, key)?;
    match_candidates(&candidates, Some(packages), &hash_hex, &mut CycleProbe::new(snapshots, key))
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
    snapshots: &HashMap<PkgNameVerPeer, SnapshotEntry>,
    snapshot: &SnapshotEntry,
    package: &PackageMetadata,
    self_key: &PkgNameVerPeer,
) -> Option<Vec<Vec<PkgNameVerPeer>>> {
    let peers = peer_names_of_snapshot(snapshot, package);
    let self_identity = self_key.without_peer();
    let root_transitive: HashSet<&str> = snapshot.transitive_peer_dependencies
        .iter()
        .flatten()
        .map(String::as_str)
        .collect();
    let transitive_peers: Vec<(PkgName, &str)> = peers
        .iter()
        .filter(|(_, name)| root_transitive.contains(name))
        .cloned()
        .collect();
    let descendant_targets = if transitive_peers.is_empty() {
        HashMap::new()
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
    if let Some(target) = resolved_peer_target(snapshot, peer) {
        push_unique_target(&mut targets, target);
    }
    for target in descendants.into_iter().flatten() {
        push_unique_target(&mut targets, target.clone());
    }
    targets
}

fn match_candidates(
    candidates: &[Vec<PkgNameVerPeer>],
    packages: Option<&HashMap<PkgNameVerPeer, PackageMetadata>>,
    hash_hex: &str,
    cycle_probe: &mut CycleProbe<'_>,
) -> Option<HashSet<PkgNameVerPeer>> {
    for mode in [PeerGroups::Full, PeerGroups::Bare] {
        if let Some(matched) = CombinationSearch::new(mode, hash_hex).find(candidates) {
            return Some(matched);
        }
    }
    if let Some(packages) = packages
        && let Some(matched) =
            CombinationSearch::new(PeerGroups::Pair(packages), hash_hex).find(candidates)
    {
        return Some(matched);
    }
    // Cycle cuts can omit nested groups (and collapse to name@version) for cyclic providers.
    let (mixed, changed) = mixed_cycle_cut_candidates(candidates, packages, cycle_probe);
    if !changed {
        return None;
    }
    CombinationSearch::new(PeerGroups::Full, hash_hex).find_mixed(&mixed)
}

fn mixed_cycle_cut_candidates(
    candidates: &[Vec<PkgNameVerPeer>],
    packages: Option<&HashMap<PkgNameVerPeer, PackageMetadata>>,
    cycle_probe: &mut CycleProbe<'_>,
) -> (Vec<Vec<MixedCandidate>>, bool) {
    let mut changed = false;
    let mixed = candidates
        .iter()
        .map(|targets| expand_slot_variants(targets, packages, cycle_probe, &mut changed))
        .collect();
    (mixed, changed)
}

fn expand_slot_variants(
    targets: &[PkgNameVerPeer],
    packages: Option<&HashMap<PkgNameVerPeer, PackageMetadata>>,
    cycle_probe: &mut CycleProbe<'_>,
    changed: &mut bool,
) -> Vec<MixedCandidate> {
    let mut variants: Vec<MixedCandidate> = targets
        .iter()
        .map(|target| MixedCandidate { provider: target.clone(), segment: target.to_string() })
        .collect();
    let mut seen: HashSet<String> = variants
        .iter()
        .map(|entry| entry.segment.clone())
        .collect();
    for target in targets {
        let bare = target.without_peer();
        let bare_seg = bare.to_string();
        let pair_seg = packages.map(|pkgs| pair_segment_of(target, pkgs));
        let has_cut_form = !target.suffix.peer().is_empty()
            || pair_seg
                .as_deref()
                .is_some_and(|seg| seg != bare_seg);
        if !has_cut_form || !cycle_probe.may_be_cut(target) {
            continue;
        }
        if seen.insert(bare_seg.clone()) {
            *changed = true;
            variants.push(MixedCandidate { provider: bare.clone(), segment: bare_seg });
        }
        if let Some(pair_seg) = pair_seg
            && seen.insert(pair_seg.clone())
        {
            *changed = true;
            variants.push(MixedCandidate { provider: bare, segment: pair_seg });
        }
    }
    variants
}

impl BoundProviders {
    /// The providers recorded for `key`, falling back to hashed recovery
    /// when the snapshot has no recorded bindings for that key.
    #[must_use]
    pub fn of_snapshot(entries: LockfileEntries<'_>, key: &PkgNameVerPeer) -> Self {
        let providers = Self::from_recorded_snapshot(entries, key);
        if !providers.is_hashed() {
            return providers;
        }
        match recover_hashed_providers(entries, key) {
            Some(recovered) => Self::from_providers(recovered),
            None => providers,
        }
    }
}

#[cfg(test)]
mod tests;
