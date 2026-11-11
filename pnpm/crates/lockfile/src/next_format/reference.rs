//! A reference of the next format: a version, `name: version`. Where the
//! referrer's scope does not say which snapshot of that version it links,
//! the reference names the snapshot by the peer bindings that tell the
//! package's snapshots apart: `name: {version, with: {peer: version}}`.
//! No reference carries a peer or patch group.

use crate::{PkgName, PkgNameVerPeer, SnapshotDepRef};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor},
};
use std::collections::BTreeMap;

/// The bindings that name a snapshot: the peer's name, and the reference the
/// snapshot binds it to, without groups.
pub type NamedBindings = BTreeMap<String, NextRef>;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NextRef {
    Plain(SnapshotDepRef),
    Named {
        version: SnapshotDepRef,
        #[serde(rename = "with")]
        peers: NamedBindings,
    },
}

/// The map form of a reference.
#[derive(Deserialize)]
struct NamedWire {
    version: SnapshotDepRef,
    #[serde(rename = "with")]
    peers: NamedBindings,
}

/// A string is a plain reference and a map a named one. Reading the two
/// shapes directly spares serde the buffering an untagged enum needs, on
/// every reference of the file.
impl<'de> Deserialize<'de> for NextRef {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        struct RefVisitor;

        impl<'de> Visitor<'de> for RefVisitor {
            type Value = NextRef;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(
                    "a version, or a map of version and the bindings that name the snapshot",
                )
            }

            fn visit_str<Error: de::Error>(self, text: &str) -> Result<NextRef, Error> {
                SnapshotDepRef::try_from(std::borrow::Cow::Borrowed(text))
                    .map(NextRef::Plain)
                    .map_err(Error::custom)
            }

            fn visit_map<Map: MapAccess<'de>>(self, map: Map) -> Result<NextRef, Map::Error> {
                let NamedWire { version, peers } =
                    NamedWire::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(NextRef::Named { version, peers })
            }
        }

        deserializer.deserialize_any(RefVisitor)
    }
}

impl NextRef {
    #[must_use]
    pub fn version(&self) -> &SnapshotDepRef {
        match self {
            NextRef::Plain(version) | NextRef::Named { version, .. } => version,
        }
    }

    /// The bindings that name the snapshot, when the reference carries them.
    #[must_use]
    pub fn peers(&self) -> Option<&NamedBindings> {
        match self {
            NextRef::Plain(_) => None,
            NextRef::Named { peers, .. } => Some(peers),
        }
    }

    /// The package key the reference names, without groups.
    #[must_use]
    pub fn resolve(&self, alias: &PkgName) -> Option<PkgNameVerPeer> {
        self.version().resolve(alias)
    }

    #[must_use]
    pub fn as_link_target(&self) -> Option<&str> {
        self.version().as_link_target()
    }
}

/// `reference` with its peer and patch groups dropped.
#[must_use]
pub fn without_groups(reference: &SnapshotDepRef) -> SnapshotDepRef {
    match reference {
        SnapshotDepRef::Plain(version) => SnapshotDepRef::Plain(version.without_peer()),
        SnapshotDepRef::Alias(key) => SnapshotDepRef::Alias(key.without_peer()),
        SnapshotDepRef::Link(path) => SnapshotDepRef::Link(path.clone()),
    }
}

/// Matches the version and any explicit provider bindings in `wanted`.
#[must_use]
pub fn names_binding(wanted: &NextRef, binding: &NextRef) -> bool {
    *wanted.version() == without_groups(binding.version())
        && wanted
            .peers()
            .is_none_or(|wanted| match binding.peers() {
                Some(actual) => actual == wanted,
                None => wanted.is_empty(),
            })
}
