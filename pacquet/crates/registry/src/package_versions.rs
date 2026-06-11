//! Lazily-hydrated view of a packument's `versions` map.
//!
//! Hydrating every version of a multi-thousand-release packument into
//! typed [`PackageVersion`]s dominated resolve CPU: the maps, strings,
//! and `serde_json::Value` trees behind each version are built, hashed,
//! and dropped even though a pick consults only the version *strings*
//! plus the handful of manifests it actually considers. Each version
//! therefore stays as the raw JSON fragment serde captured — an
//! [`Arc<RawValue>`], shared rather than copied — until someone asks
//! for the typed form, and the hydrated manifest is cached per slot so
//! repeated lookups parse once.
//!
//! A fragment that fails to decode behaves as if the version were
//! absent from the packument (with a `tracing::warn`), mirroring the
//! tolerance of JavaScript package managers, which never validate
//! version entries they don't pick.

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::package_version::PackageVersion;

#[derive(Debug, Default, Clone)]
pub struct PackageVersions {
    slots: HashMap<String, VersionSlot>,
}

#[derive(Debug)]
struct VersionSlot {
    /// Raw JSON fragment as served by the registry. `None` only for
    /// slots constructed from an already-typed manifest (tests, the
    /// publish-date filter's slot moves).
    raw: Option<Arc<RawValue>>,
    /// Hydration cache. `Some(None)` records a fragment that failed
    /// to decode so the parse error is paid (and warned about) once.
    parsed: OnceLock<Option<Arc<PackageVersion>>>,
}

impl Clone for VersionSlot {
    fn clone(&self) -> Self {
        VersionSlot {
            raw: self.raw.clone(),
            parsed: match self.parsed.get() {
                Some(value) => OnceLock::from(value.clone()),
                None => OnceLock::new(),
            },
        }
    }
}

impl VersionSlot {
    fn from_parsed(manifest: PackageVersion) -> Self {
        VersionSlot { raw: None, parsed: OnceLock::from(Some(Arc::new(manifest))) }
    }

    fn hydrate(&self, version: &str) -> Option<Arc<PackageVersion>> {
        self.parsed
            .get_or_init(|| {
                let raw = self.raw.as_ref()?;
                match serde_json::from_str::<PackageVersion>(raw.get()) {
                    Ok(parsed) => Some(Arc::new(parsed)),
                    Err(error) => {
                        tracing::warn!(
                            target: "pacquet_registry",
                            %error,
                            version,
                            "skipping registry version with an undecodable manifest",
                        );
                        None
                    }
                }
            })
            .clone()
    }
}

impl PackageVersions {
    /// Typed manifest for `version`, hydrating the raw fragment on
    /// first access. `None` when the version is absent *or* its
    /// fragment fails to decode.
    #[must_use]
    pub fn get(&self, version: &str) -> Option<Arc<PackageVersion>> {
        self.slots.get(version)?.hydrate(version)
    }

    /// Whether the packument lists `version`. Never hydrates.
    #[must_use]
    pub fn contains_key(&self, version: &str) -> bool {
        self.slots.contains_key(version)
    }

    /// Version strings, in `HashMap` order. Never hydrates.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.slots.keys()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Iterate `(version, manifest)` pairs, hydrating every fragment.
    /// Undecodable fragments are skipped. Full walks defeat the lazy
    /// representation, so this belongs only on cold paths (the trust
    /// verifier's history scan, tests).
    pub fn iter(&self) -> impl Iterator<Item = (&String, Arc<PackageVersion>)> {
        self.slots.iter().filter_map(|(version, slot)| Some((version, slot.hydrate(version)?)))
    }

    /// Filtered copy keeping only the versions `keep` accepts. Slots
    /// move as fragments — nothing hydrates. Used by the
    /// publish-date filter, which decides on the packument's `time`
    /// map rather than the manifests.
    #[must_use]
    pub fn filtered(&self, mut keep: impl FnMut(&str) -> bool) -> PackageVersions {
        PackageVersions {
            slots: self
                .slots
                .iter()
                .filter(|(version, _)| keep(version))
                .map(|(version, slot)| (version.clone(), slot.clone()))
                .collect(),
        }
    }
}

impl From<HashMap<String, PackageVersion>> for PackageVersions {
    fn from(versions: HashMap<String, PackageVersion>) -> Self {
        PackageVersions {
            slots: versions
                .into_iter()
                .map(|(version, manifest)| (version, VersionSlot::from_parsed(manifest)))
                .collect(),
        }
    }
}

impl FromIterator<(String, PackageVersion)> for PackageVersions {
    fn from_iter<Iter: IntoIterator<Item = (String, PackageVersion)>>(iter: Iter) -> Self {
        iter.into_iter().collect::<HashMap<_, _>>().into()
    }
}

impl<'de> Deserialize<'de> for PackageVersions {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        let raw_map = HashMap::<String, Box<RawValue>>::deserialize(deserializer)?;
        Ok(PackageVersions {
            slots: raw_map
                .into_iter()
                .map(|(version, raw)| {
                    (version, VersionSlot { raw: Some(Arc::from(raw)), parsed: OnceLock::new() })
                })
                .collect(),
        })
    }
}

impl Serialize for PackageVersions {
    fn serialize<Ser: Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.slots.len()))?;
        for (version, slot) in &self.slots {
            match (&slot.raw, slot.parsed.get()) {
                // Raw fragments round-trip verbatim — re-serializing a
                // hydrated manifest would reorder keys and drop nothing
                // but risks shape drift; the wire bytes are canonical.
                (Some(raw), _) => map.serialize_entry(version, raw.as_ref())?,
                (None, Some(Some(parsed))) => map.serialize_entry(version, parsed.as_ref())?,
                // Unreachable by construction (a slot always has a raw
                // fragment or a pre-set parsed manifest); skip rather
                // than panic inside a serializer.
                (None, _) => {}
            }
        }
        map.end()
    }
}

#[cfg(test)]
mod tests;
