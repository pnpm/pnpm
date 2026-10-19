use crate::PkgNameVerPeer;
use pnpm_deps_path::{DepPath, PeerId, create_peer_dep_graph_hash};

/// The peer providers a snapshot key records in its peer suffix. The
/// suffix is the only record of the providers peer resolution bound,
/// including where it cut a peer cycle short, so every reader goes
/// through [`PeerBindings::from_key`].
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

#[cfg(test)]
mod tests;
