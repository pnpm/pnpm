use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::PkgNameVerPeer;
use pnpm_deps_path::{DepPath, PeerId, create_peer_dep_graph_hash, index_of_dep_path_suffix};

/// Providers recorded before a snapshot's peer suffix was hashed.
#[derive(Debug, Clone)]
pub struct RecordedPeerBindings {
    key: PkgNameVerPeer,
    providers: BoundProviders,
}

impl RecordedPeerBindings {
    /// `peers` must be the values used to construct `dep_path`.
    #[must_use]
    pub fn for_dep_path(dep_path: &DepPath, peers: &[PeerId]) -> Option<Arc<Self>> {
        let text = dep_path.as_str();
        let start = index_of_dep_path_suffix(text).peers_index?;
        let hash = text[start..].strip_prefix('(')?.strip_suffix(')')?;
        if !is_peer_suffix_hash(hash) {
            return None;
        }
        Self::for_key(&text.parse().ok()?, peers)
    }

    /// `peers` must be the values used to construct `key`.
    pub(crate) fn for_key(key: &PkgNameVerPeer, peers: &[PeerId]) -> Option<Arc<Self>> {
        let hash = key.suffix
            .peer()
            .strip_prefix(key.suffix.patch_group())?
            .strip_prefix('(')?
            .strip_suffix(')')?;
        if !is_peer_suffix_hash(hash) {
            return None;
        }
        let bindings = PeerBindings::Explicit(
            peers
                .iter()
                .map(PeerId::as_segment)
                .collect(),
        );
        Some(Arc::new(Self { key: key.clone(), providers: BoundProviders::of(&bindings) }))
    }

    pub(crate) fn providers_for(&self, key: &PkgNameVerPeer) -> Option<&BoundProviders> {
        (key == &self.key).then_some(&self.providers)
    }
}

/// The providers one key's peer bindings name. Provider groups may omit
/// their own peers (`dedupePeers`). Unrecovered hashes name no providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundProviders(Option<HashMap<PkgNameVerPeer, HashSet<PkgNameVerPeer>>>);

impl BoundProviders {
    /// The providers `key`'s peer suffix names. Empty when `key` has no
    /// peer suffix.
    #[must_use]
    pub fn from_key(key: &PkgNameVerPeer) -> Self {
        match PeerBindings::from_key(key) {
            Some(bindings) => Self::of(&bindings),
            None => Self::from_providers([]),
        }
    }

    /// The providers `bindings` name.
    #[must_use]
    pub fn of(bindings: &PeerBindings) -> Self {
        match bindings {
            PeerBindings::Hashed => BoundProviders(None),
            PeerBindings::Explicit(_) => Self::from_providers(bindings.providers()),
        }
    }

    fn from_providers(providers: impl IntoIterator<Item = PkgNameVerPeer>) -> Self {
        let mut indexed: HashMap<_, HashSet<_>> = HashMap::new();
        for provider in providers {
            indexed
                .entry(provider.without_peer())
                .or_default()
                .insert(provider);
        }
        BoundProviders(Some(indexed))
    }

    /// Whether the suffix was hashed (`peersSuffixMaxLength`) rather than
    /// listing explicit providers.
    #[must_use]
    pub fn is_hashed(&self) -> bool {
        self.0.is_none()
    }

    /// Whether the bindings name `target`. Every target counts for a
    /// hashed suffix.
    #[must_use]
    pub fn binds(&self, target: &PkgNameVerPeer) -> bool {
        self.0
            .as_ref()
            .is_none_or(|providers| providers.contains_key(&target.without_peer()))
    }

    /// The recorded keys for `target`'s package identity, including any cycle cut.
    /// `None` when the target is unbound or the hash could not be recovered.
    #[must_use]
    pub fn provider_keys(&self, target: &PkgNameVerPeer) -> Option<&HashSet<PkgNameVerPeer>> {
        self.0
            .as_ref()?
            .get(&target.without_peer())
    }

    /// Matches the full provider key when recorded, or any peer context when
    /// the recorded group omits the provider's peers.
    pub(crate) fn binds_snapshot(&self, target: &PkgNameVerPeer) -> bool {
        let bare = target.without_peer();
        self.0
            .as_ref()
            .is_none_or(|providers| {
                providers
                    .get(&bare)
                    .is_some_and(|keys| keys.contains(target) || keys.contains(&bare))
            })
    }
}

/// The peer providers encoded in a snapshot key, including cycle cuts.
/// Use [`BoundProviders::of_snapshot`] when the snapshot is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerBindings {
    /// The spelled-out segments, in suffix order. A segment is a
    /// provider's key, or a `patch_hash=` marker.
    Explicit(Vec<String>),
    /// The opaque hash written in place of the provider segments once they
    /// exceed `peersSuffixMaxLength`. It names no provider. A `patch_hash=`
    /// marker before it is not part of the hash.
    Hashed,
}

impl PeerBindings {
    /// The bindings `key`'s peer suffix records. `None` when it has no
    /// suffix, or the suffix is not a flat run of balanced groups.
    #[must_use]
    pub fn from_key(key: &PkgNameVerPeer) -> Option<Self> {
        Self::from_suffix(key.suffix.peer())
    }

    /// The bindings a peer suffix such as `(a@1.0.0)(b@2.0.0(c@1.0.0))`
    /// records. See [`Self::from_key`].
    #[must_use]
    pub fn from_suffix(peer_suffix: &str) -> Option<Self> {
        if peer_suffix.is_empty() {
            return None;
        }
        let segments = split_peer_suffix_segments(peer_suffix)?;
        let mut provider_segments = segments
            .iter()
            .filter(|segment| !is_patch_marker(segment));
        if let (Some(only), None) = (provider_segments.next(), provider_segments.next())
            && is_peer_suffix_hash(only)
        {
            return Some(PeerBindings::Hashed);
        }
        Some(PeerBindings::Explicit(segments))
    }

    /// The peer suffix that records the providers `peer_ids` that peer
    /// resolution bound, hashed when the segments exceed `max_length`
    /// (`peersSuffixMaxLength`). [`Self::from_suffix`] reads it back.
    #[must_use]
    pub fn suffix_of(peer_ids: &[PeerId], max_length: usize) -> String {
        create_peer_dep_graph_hash(peer_ids, max_length)
    }

    /// The provider keys the segments name, in suffix order. Empty for
    /// [`PeerBindings::Hashed`].
    pub fn providers(&self) -> impl Iterator<Item = PkgNameVerPeer> + '_ {
        let segments = match self {
            PeerBindings::Explicit(segments) => segments.as_slice(),
            PeerBindings::Hashed => &[],
        };
        segments
            .iter()
            .filter_map(|segment| segment.parse().ok())
    }

    /// The peer suffix that records these bindings; see
    /// [`Self::suffix_of`]. `patch_hash=` markers stay in front, verbatim,
    /// and only the provider segments are sorted and hashed. `None` for
    /// [`PeerBindings::Hashed`], whose segments are gone.
    #[must_use]
    pub fn to_suffix(&self, max_length: usize) -> Option<String> {
        let PeerBindings::Explicit(segments) = self else { return None };
        let (patch_markers, providers): (Vec<&String>, Vec<&String>) =
            segments.iter().partition(|segment| is_patch_marker(segment));
        let mut suffix = String::new();
        for marker in patch_markers {
            suffix.push('(');
            suffix.push_str(marker);
            suffix.push(')');
        }
        if !providers.is_empty() {
            let peer_ids: Vec<PeerId> = providers
                .into_iter()
                .map(|segment| PeerId::DepPath(DepPath::from(segment.clone())))
                .collect();
            suffix.push_str(&Self::suffix_of(&peer_ids, max_length));
        }
        Some(suffix)
    }
}

/// Splits a peer suffix into its segment bodies. `None` when the suffix is
/// anything but a flat run of balanced parenthesised groups.
#[must_use]
pub fn split_peer_suffix_segments(suffix: &str) -> Option<Vec<String>> {
    let mut segments = Vec::new();
    let mut rest = suffix;
    while !rest.is_empty() {
        let body = rest.strip_prefix('(')?;
        let close = matching_close_paren(body)?;
        segments.push(body[..close].to_string());
        rest = &body[close + 1..];
    }
    Some(segments)
}

/// Index of the `)` that closes a group whose `(` precedes `body`.
fn matching_close_paren(body: &str) -> Option<usize> {
    let mut depth = 0_u32;
    for (idx, byte) in body.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' if depth == 0 => return Some(idx),
            b')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Whether a segment is the hash [`PeerBindings::suffix_of`] writes for an
/// over-long suffix.
fn is_patch_marker(segment: &str) -> bool {
    segment.starts_with("patch_hash=")
}

fn is_peer_suffix_hash(segment: &str) -> bool {
    segment.len() == 32 && segment.chars().all(|character| character.is_ascii_hexdigit())
}

mod hashed;

#[cfg(test)]
mod tests;
