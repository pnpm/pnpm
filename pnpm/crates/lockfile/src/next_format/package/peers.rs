//! The one `peerDependencies` map of an entry. It holds what today's
//! `peerDependencies` and `peerDependenciesMeta` hold, and the peer bindings:
//! the package's own, beside the range they satisfy, and those a dependency
//! below needs, marked `below: true`.

use super::super::{NamedBindings, NextFormatError, NextRef};
use crate::{PeerDependencyMeta, PkgName, SnapshotDepRef};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor},
};
use std::collections::{BTreeMap, HashMap};

/// One entry of `peerDependencies`: the declared range alone, or a map of
/// what there is to say about the peer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PeerEntry {
    Specifier(String),
    Detailed(Box<PeerDetails>),
}

/// A string is the specifier and a map the details, read directly rather
/// than through an untagged enum's buffering.
impl<'de> Deserialize<'de> for PeerEntry {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        struct EntryVisitor;

        impl<'de> Visitor<'de> for EntryVisitor {
            type Value = PeerEntry;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a specifier, or a map describing the peer")
            }

            fn visit_str<Error: de::Error>(self, text: &str) -> Result<PeerEntry, Error> {
                Ok(PeerEntry::Specifier(text.to_owned()))
            }

            fn visit_string<Error: de::Error>(self, text: String) -> Result<PeerEntry, Error> {
                Ok(PeerEntry::Specifier(text))
            }

            fn visit_map<Map: MapAccess<'de>>(self, map: Map) -> Result<PeerEntry, Map::Error> {
                PeerDetails::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(|details| PeerEntry::Detailed(Box::new(details)))
            }
        }

        deserializer.deserialize_any(EntryVisitor)
    }
}

/// A snapshot's binding as written, borrowing the binding.
#[derive(Serialize)]
pub(in crate::next_format) struct BindingOut<'a> {
    version: &'a SnapshotDepRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    peers: Option<&'a NamedBindings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    below: Option<bool>,
}

/// A peer written as a map. `specifier` is the range the package declares,
/// `version` the binding, `peers` what names the binding's snapshot when its
/// version does not, `optional` what `peerDependenciesMeta` says, and
/// `below` marks a peer the package does not declare: a dependency below it
/// does, and the binding is this package's to pass down.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<SnapshotDepRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peers: Option<NamedBindings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub below: Option<bool>,
}

impl PeerDetails {
    fn of_binding(binding: &NextRef, below: bool) -> Self {
        PeerDetails {
            version: Some(binding.version().clone()),
            peers: binding.peers().cloned(),
            below: below.then_some(true),
            ..PeerDetails::default()
        }
    }

    /// The binding, when the entry has one.
    fn binding(&self) -> Option<NextRef> {
        let version = self.version.clone()?;
        Some(match self.peers.clone() {
            Some(peers) => NextRef::Named { version, peers },
            None => NextRef::Plain(version),
        })
    }
}

/// The bindings an entry or a snapshot carries, split as the in-memory
/// snapshot holds them.
#[derive(Default)]
pub(in crate::next_format) struct Bindings {
    pub own: Option<HashMap<PkgName, NextRef>>,
    pub below: Option<HashMap<PkgName, NextRef>>,
}

impl Bindings {
    /// The `peerDependencies` map of a snapshot: its bindings alone.
    pub(in crate::next_format) fn entries<'a>(
        own: Option<&'a HashMap<PkgName, NextRef>>,
        below: Option<&'a HashMap<PkgName, NextRef>>,
    ) -> Option<BTreeMap<String, BindingOut<'a>>> {
        let binding_out = |(name, binding): (&'a PkgName, &'a NextRef), below: bool| {
            let out = BindingOut {
                version: binding.version(),
                peers: binding.peers(),
                below: below.then_some(true),
            };
            (name.to_string(), out)
        };
        let entries: BTreeMap<String, BindingOut<'a>> = own
            .into_iter()
            .flatten()
            .map(|binding| binding_out(binding, false))
            .chain(
                below
                    .into_iter()
                    .flatten()
                    .map(|binding| binding_out(binding, true)),
            )
            .collect();
        (!entries.is_empty()).then_some(entries)
    }

    /// The bindings among `entries`, keyed by package name already.
    pub(in crate::next_format) fn of_entries(entries: HashMap<PkgName, PeerEntry>) -> Self {
        let mut bindings = Bindings::default();
        for (name, entry) in entries {
            if let PeerEntry::Detailed(details) = entry {
                bindings.add(name, &details);
            }
        }
        bindings
    }

    fn add(&mut self, name: PkgName, details: &PeerDetails) {
        let Some(binding) = details.binding() else { return };
        let map = if details.below == Some(true) { &mut self.below } else { &mut self.own };
        map.get_or_insert_default().insert(name, binding);
    }
}

fn add_bindings(
    entries: &mut BTreeMap<String, PeerEntry>,
    bindings: Option<&HashMap<PkgName, NextRef>>,
    below: bool,
) {
    for (name, binding) in bindings.into_iter().flatten() {
        let details = PeerDetails::of_binding(binding, below);
        match entries.get_mut(&name.to_string()) {
            Some(PeerEntry::Detailed(existing)) => {
                existing.version = details.version;
                existing.peers = details.peers;
                existing.below = details.below;
            }
            Some(entry @ PeerEntry::Specifier(_)) => {
                let specifier = match entry {
                    PeerEntry::Specifier(specifier) => Some(std::mem::take(specifier)),
                    PeerEntry::Detailed(_) => None,
                };
                *entry = PeerEntry::Detailed(Box::new(PeerDetails { specifier, ..details }));
            }
            None => {
                entries.insert(name.to_string(), PeerEntry::Detailed(Box::new(details)));
            }
        }
    }
}

/// Today's declared peers and their meta, with the bindings every snapshot
/// shares, as the one map the entry writes.
pub(super) fn merge_peer_dependencies(
    ranges: Option<&HashMap<String, String>>,
    meta: Option<&HashMap<String, PeerDependencyMeta>>,
    shared: &super::NextSnapshotEntry,
) -> Option<BTreeMap<String, PeerEntry>> {
    let mut entries: BTreeMap<String, PeerEntry> = ranges
        .into_iter()
        .flatten()
        .map(|(name, range)| (name.clone(), PeerEntry::Specifier(range.clone())))
        .collect();
    for (name, PeerDependencyMeta { optional }) in meta.into_iter().flatten() {
        let specifier = ranges
            .and_then(|ranges| ranges.get(name))
            .cloned();
        let details =
            PeerDetails { specifier, optional: Some(*optional), ..PeerDetails::default() };
        entries.insert(name.clone(), PeerEntry::Detailed(Box::new(details)));
    }
    add_bindings(&mut entries, shared.peer_dependencies.as_ref(), false);
    add_bindings(&mut entries, shared.transitive_peer_dependencies.as_ref(), true);
    (!entries.is_empty()).then_some(entries)
}

/// The entry's one peer map as today's parts.
pub(super) struct SplitPeers {
    pub ranges: Option<HashMap<String, String>>,
    pub meta: Option<HashMap<String, PeerDependencyMeta>>,
    pub bindings: Bindings,
}

/// Fails on a bound peer whose name is not a package name.
pub(super) fn split_peer_dependencies(
    key: &crate::PackageKey,
    merged: Option<HashMap<String, PeerEntry>>,
) -> Result<SplitPeers, NextFormatError> {
    let mut ranges = HashMap::new();
    let mut meta = HashMap::new();
    let mut bindings = Bindings::default();
    for (name, entry) in merged.into_iter().flatten() {
        let details = match entry {
            PeerEntry::Specifier(range) => {
                ranges.insert(name, range);
                continue;
            }
            PeerEntry::Detailed(details) => details,
        };
        if let Some(optional) = details.optional {
            meta.insert(name.clone(), PeerDependencyMeta { optional });
        }
        if let Some(specifier) = details.specifier.clone() {
            ranges.insert(name.clone(), specifier);
        }
        if details.version.is_some() {
            let parsed = PkgName::parse(&name)
                .map_err(|_| NextFormatError::InvalidPeerName {
                    referrer: key.to_string(),
                    name: name.clone(),
                })?;
            bindings.add(parsed, &details);
        }
    }
    Ok(SplitPeers {
        ranges: (!ranges.is_empty()).then_some(ranges),
        meta: (!meta.is_empty()).then_some(meta),
        bindings,
    })
}
