use std::{
    borrow::Cow,
    collections::{
        HashMap,
        HashSet,
    },
};

use pnpm_crypto_hash::create_short_hash;

use crate::{
    PackageMetadata,
    PkgNameVerPeer,
};

pub(super) const MAX_COMBINATIONS: usize = 4096;

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct MixedCandidate {
    pub(super) provider: PkgNameVerPeer,
    pub(super) segment: String,
}

#[derive(Clone, Copy)]
pub(super) enum PeerGroups<'pkg> {
    Full,
    Bare,
    Pair(&'pkg HashMap<PkgNameVerPeer, PackageMetadata>),
}

impl PeerGroups<'_> {
    fn provider(self, target: &PkgNameVerPeer) -> Cow<'_, PkgNameVerPeer> {
        match self {
            Self::Full => Cow::Borrowed(target),
            Self::Bare | Self::Pair(_) => Cow::Owned(target.without_peer()),
        }
    }

    fn segment(self, target: &PkgNameVerPeer) -> String {
        match self {
            Self::Full => target.to_string(),
            Self::Bare => target.without_peer().to_string(),
            Self::Pair(packages) => pair_segment_of(target, packages),
        }
    }
}

pub(super) fn pair_segment_of(
    target: &PkgNameVerPeer,
    packages: &HashMap<PkgNameVerPeer, PackageMetadata>,
) -> String {
    let bare = target.without_peer();
    let (name, version) = crate::name_version_from_package_key(&bare, Some(packages));
    match bare.suffix.registry_qualified() {
        Some((registry_name, _)) => format!("{name}@{registry_name}:{version}"),
        None => format!("{name}@{version}"),
    }
}

pub(super) struct CombinationSearch<'hash, 'pkg> {
    mode: PeerGroups<'pkg>,
    hash_hex: &'hash str,
    pub(super) remaining: usize,
}

impl<'hash, 'pkg> CombinationSearch<'hash, 'pkg> {
    pub(super) fn new(mode: PeerGroups<'pkg>, hash_hex: &'hash str) -> Self {
        Self { mode, hash_hex, remaining: MAX_COMBINATIONS }
    }

    pub(super) fn find(
        &mut self,
        candidates: &[Vec<PkgNameVerPeer>],
    ) -> Option<HashSet<PkgNameVerPeer>> {
        let mapped: Vec<Vec<MixedCandidate>> = candidates
            .iter()
            .map(|targets| deduplicate_slot(targets, self.mode))
            .collect();
        self.find_mixed(&mapped)
    }

    pub(super) fn find_mixed(
        &mut self,
        candidates: &[Vec<MixedCandidate>],
    ) -> Option<HashSet<PkgNameVerPeer>> {
        let deduped: Vec<Vec<MixedCandidate>> = candidates
            .iter()
            .map(|slot| deduplicate_mixed_slot(slot))
            .collect();
        let mut chosen = Vec::with_capacity(deduped.len());
        // Prefer complete provider sets before spending the budget on omissions.
        for omitted in 0..=deduped.len() {
            if self.remaining == 0 {
                break;
            }
            if let Some(matched) = self.with_omissions(&deduped, &mut chosen, omitted) {
                return Some(matched);
            }
        }
        None
    }

    fn with_omissions<'a>(
        &mut self,
        candidates: &'a [Vec<MixedCandidate>],
        chosen: &mut Vec<&'a MixedCandidate>,
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

    fn match_chosen(&mut self, chosen: &[&MixedCandidate]) -> Option<HashSet<PkgNameVerPeer>> {
        self.remaining -= 1;
        subset_matches_hash(chosen, self.hash_hex)
            .then(|| {
                chosen
                    .iter()
                    .map(|entry| entry.provider.clone())
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
        targets: &'a [MixedCandidate],
        chosen: &mut Vec<&'a MixedCandidate>,
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

fn deduplicate_slot(targets: &[PkgNameVerPeer], mode: PeerGroups<'_>) -> Vec<MixedCandidate> {
    let mut seen = HashSet::with_capacity(targets.len());
    targets
        .iter()
        .map(|target| MixedCandidate {
            provider: mode.provider(target).into_owned(),
            segment: mode.segment(target),
        })
        .filter(|candidate| seen.insert(candidate.clone()))
        .collect()
}

fn deduplicate_mixed_slot(slot: &[MixedCandidate]) -> Vec<MixedCandidate> {
    let mut seen = HashSet::with_capacity(slot.len());
    slot.iter()
        .filter(|candidate| seen.insert(*candidate))
        .cloned()
        .collect()
}

fn subset_matches_hash(chosen: &[&MixedCandidate], hash_hex: &str) -> bool {
    if chosen.is_empty() {
        return false;
    }
    let mut segments: Vec<&str> = chosen
        .iter()
        .map(|entry| entry.segment.as_str())
        .collect();
    segments.sort_unstable();
    create_short_hash(&segments.join(")(")) == hash_hex
}
