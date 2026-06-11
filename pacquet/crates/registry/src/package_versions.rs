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
    borrow::Cow,
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
    source: FragmentSource,
    /// Hydration cache. `Some(None)` records a fragment that failed
    /// to decode so the parse error is paid (and warned about) once.
    parsed: OnceLock<Option<Arc<PackageVersion>>>,
}

/// Where a version's JSON fragment lives until it is hydrated.
#[derive(Debug, Clone)]
enum FragmentSource {
    /// Raw JSON fragment as served by the registry (the serde parse
    /// of a packument body captures these).
    Raw(Arc<RawValue>),
    /// Byte span inside an indexed on-disk metadata mirror, whose
    /// contents the loader read into one shared buffer. Hydration
    /// parses the span in place — no per-fragment file I/O (the pick
    /// paths can probe many candidate fragments per package, so
    /// open-per-hydration measured slower than one sequential read).
    BufferSpan { buffer: Arc<Vec<u8>>, offset: u64, len: u32 },
    /// No fragment — the slot was constructed from an already-typed
    /// manifest (tests, the publish-date filter's slot moves).
    None,
}

impl FragmentSource {
    /// The fragment's JSON text: borrowed for [`FragmentSource::Raw`],
    /// read from the shared buffer for [`FragmentSource::BufferSpan`],
    /// absent for [`FragmentSource::None`] or invalid spans.
    fn json(&self) -> Option<Cow<'_, str>> {
        match self {
            FragmentSource::Raw(raw) => Some(Cow::Borrowed(raw.get())),
            FragmentSource::BufferSpan { buffer, offset, len } => {
                let start = usize::try_from(*offset).ok()?;
                let end = start.checked_add(*len as usize)?;
                let bytes = buffer.get(start..end)?;
                match std::str::from_utf8(bytes) {
                    Ok(json) => Some(Cow::Borrowed(json)),
                    Err(error) => {
                        tracing::warn!(
                            target: "pacquet_registry",
                            %error,
                            offset,
                            "metadata mirror fragment is not valid UTF-8",
                        );
                        None
                    }
                }
            }
            FragmentSource::None => None,
        }
    }
}

impl Clone for VersionSlot {
    fn clone(&self) -> Self {
        VersionSlot {
            source: self.source.clone(),
            parsed: match self.parsed.get() {
                Some(value) => OnceLock::from(value.clone()),
                None => OnceLock::new(),
            },
        }
    }
}

impl VersionSlot {
    fn from_parsed(manifest: PackageVersion) -> Self {
        VersionSlot {
            source: FragmentSource::None,
            parsed: OnceLock::from(Some(Arc::new(manifest))),
        }
    }

    fn hydrate(&self, version: &str) -> Option<Arc<PackageVersion>> {
        self.parsed
            .get_or_init(|| {
                let json = self.source.json()?;
                match serde_json::from_str::<PackageVersion>(&json) {
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

/// Constructors and accessors for the indexed on-disk mirror format
/// (see `pacquet-resolving-npm-resolver`'s `mirror` module, which owns
/// the file layout).
impl PackageVersions {
    /// Build a map whose fragments are byte spans inside `buffer`
    /// (the indexed mirror file's contents). Nothing parses until a
    /// version hydrates.
    #[must_use]
    pub fn from_buffer_spans(
        buffer: &Arc<Vec<u8>>,
        spans: impl IntoIterator<Item = (String, u64, u32)>,
    ) -> Self {
        PackageVersions {
            slots: spans
                .into_iter()
                .map(|(version, offset, len)| {
                    (
                        version,
                        VersionSlot {
                            source: FragmentSource::BufferSpan {
                                buffer: Arc::clone(buffer),
                                offset,
                                len,
                            },
                            parsed: OnceLock::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    /// Iterate every version's JSON fragment text, for the mirror
    /// writer. Raw fragments borrow; slots holding only a typed
    /// manifest re-serialize it; file-span slots read their span.
    /// A slot whose fragment can be neither borrowed nor produced is
    /// skipped with a warning — the mirror then simply omits that
    /// version, which reads back as "absent" (the same contract as an
    /// undecodable fragment).
    pub fn fragments(&self) -> impl Iterator<Item = (&String, Cow<'_, str>)> {
        self.slots.iter().filter_map(|(version, slot)| {
            if let Some(json) = slot.source.json() {
                return Some((version, json));
            }
            if let Some(Some(parsed)) = slot.parsed.get() {
                match serde_json::to_string(parsed.as_ref()) {
                    Ok(json) => return Some((version, Cow::Owned(json))),
                    Err(error) => {
                        tracing::warn!(
                            target: "pacquet_registry",
                            %error,
                            version,
                            "failed to re-serialize a typed manifest for the metadata mirror",
                        );
                    }
                }
            }
            None
        })
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
                    (
                        version,
                        VersionSlot {
                            source: FragmentSource::Raw(Arc::from(raw)),
                            parsed: OnceLock::new(),
                        },
                    )
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
            // Fragments round-trip verbatim — re-serializing a hydrated
            // manifest would reorder keys; the wire bytes are canonical.
            // File-span fragments read their span here (rare: only a
            // file-loaded packument being re-serialized).
            if let Some(json) = slot.source.json() {
                match serde_json::from_str::<&RawValue>(&json) {
                    Ok(raw) => map.serialize_entry(version, raw)?,
                    Err(error) => {
                        tracing::warn!(
                            target: "pacquet_registry",
                            %error,
                            version,
                            "skipping registry version with a corrupt fragment during serialization",
                        );
                        continue;
                    }
                }
                continue;
            }
            if let Some(Some(parsed)) = slot.parsed.get() {
                map.serialize_entry(version, parsed.as_ref())?;
            }
            // A slot with neither a readable fragment nor a typed
            // manifest serializes as absent rather than panicking
            // inside the serializer.
        }
        map.end()
    }
}

#[cfg(test)]
mod tests;
