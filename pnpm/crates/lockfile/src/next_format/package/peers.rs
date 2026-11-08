//! The one `peerDependencies` map of an entry, which holds what today's
//! `peerDependencies` and `peerDependenciesMeta` hold.

use crate::PeerDependencyMeta;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// One entry of `peerDependencies`: the declared range alone, or the range
/// with what `peerDependenciesMeta` says about the peer beside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NextPeerDependency {
    Range(String),
    Detailed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        specifier: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        optional: Option<bool>,
    },
}

/// [`NextPeerDependency`] borrowed from the metadata it is written from.
#[derive(Serialize)]
#[serde(untagged)]
pub enum NextPeerDependencyOut<'a> {
    Range(&'a str),
    Detailed {
        #[serde(skip_serializing_if = "Option::is_none")]
        specifier: Option<&'a str>,
        optional: bool,
    },
}

/// Today's two peer maps as the one map the entry writes. A peer in either
/// map has an entry; it is detailed when `peerDependenciesMeta` names it.
pub(super) fn merge_peer_dependencies<'a>(
    ranges: Option<&'a HashMap<String, String>>,
    meta: Option<&'a HashMap<String, PeerDependencyMeta>>,
) -> Option<BTreeMap<&'a str, NextPeerDependencyOut<'a>>> {
    let mut merged: BTreeMap<&str, NextPeerDependencyOut<'_>> = ranges
        .into_iter()
        .flatten()
        .map(|(name, range)| (name.as_str(), NextPeerDependencyOut::Range(range)))
        .collect();
    for (name, PeerDependencyMeta { optional }) in meta.into_iter().flatten() {
        let specifier = ranges
            .and_then(|ranges| ranges.get(name))
            .map(String::as_str);
        merged.insert(name, NextPeerDependencyOut::Detailed { specifier, optional: *optional });
    }
    (!merged.is_empty()).then_some(merged)
}

/// Today's `peerDependencies` and `peerDependenciesMeta`.
pub(super) type PeerMaps =
    (Option<HashMap<String, String>>, Option<HashMap<String, PeerDependencyMeta>>);

/// The entry's one peer map as today's two.
pub(super) fn split_peer_dependencies(
    merged: Option<HashMap<String, NextPeerDependency>>,
) -> PeerMaps {
    let mut ranges = HashMap::new();
    let mut meta = HashMap::new();
    for (name, peer) in merged.into_iter().flatten() {
        match peer {
            NextPeerDependency::Range(range) => {
                ranges.insert(name, range);
            }
            NextPeerDependency::Detailed { specifier, optional } => {
                if let Some(optional) = optional {
                    meta.insert(name.clone(), PeerDependencyMeta { optional });
                }
                if let Some(specifier) = specifier {
                    ranges.insert(name, specifier);
                }
            }
        }
    }
    ((!ranges.is_empty()).then_some(ranges), (!meta.is_empty()).then_some(meta))
}
