//! Lazily-hydrated view of a packument's `versions` map.
//!
//! Hydrating every version of a multi-thousand-release packument into
//! typed [`PackageVersion`]s dominated resolve CPU: the maps, strings,
//! and `serde_json::Value` trees behind each version are built, hashed,
//! and dropped even though a pick consults only the version *strings*
//! plus the handful of manifests it actually considers. Each version
//! therefore stays as an unhydrated fragment — the raw JSON serde
//! captured ([`Arc<RawValue>`], shared rather than copied) or a byte
//! span read on demand from the held-open mirror file — until someone
//! asks for the typed form, and the hydrated manifest is cached per
//! slot so repeated lookups parse once.
//!
//! A registry-served fragment that fails to decode behaves as if the
//! version were absent from the packument (with a `tracing::warn`),
//! mirroring the tolerance of JavaScript package managers, which never
//! validate version entries they don't pick. A fragment read out of an
//! indexed on-disk mirror is different when its bytes are not JSON at
//! all: the index vouched for the span, and the mirror stores registry
//! fragments verbatim, so such bytes mean the local file is damaged
//! rather than that the version is missing. Those are recorded in
//! [`PackageVersions::has_corrupt_mirror_fragment`], which the resolver
//! reads to treat the whole mirror as unreadable — silently resolving a
//! different version off damaged local data would be worse than the
//! refetch, and the etag lives in the intact headers record, so nothing
//! else would ever repair the file.

pub use mirror::{MirrorFile, read_exact_at};

use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use serde_json::value::RawValue;

use crate::package_version::{
    PackageVersion, PolicyFieldsProbe, VersionPolicyFields, VersionTrustMetadata,
    deserialize_deprecated_field,
};

/// Single-field view of a version manifest for
/// [`PackageVersions::is_deprecated`] — same normalization as
/// [`PackageVersion::deprecated`], every other field skipped.
#[derive(Deserialize)]
struct DeprecatedProbe {
    #[serde(default, deserialize_with = "deserialize_deprecated_field")]
    deprecated: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct PackageVersions {
    slots: Vec<(String, VersionSlot)>,
    /// Shared with every clone and filtered view, so corruption stays
    /// visible through whichever handle the resolver ends up holding.
    corrupt_mirror_fragment: Arc<AtomicBool>,
}

#[derive(Debug)]
struct VersionSlot {
    source: FragmentSource,
    /// Hydration cache. `Some(None)` records a fragment that failed
    /// to decode so the parse error is paid (and warned about) once.
    parsed: OnceLock<Option<Arc<PackageVersion>>>,
    /// [`PackageVersions::is_deprecated`] probe cache. Stays empty when
    /// the fragment could not be read, so a failed mirror read is
    /// retried. A populated `parsed` takes precedence over it.
    deprecated: OnceLock<bool>,
    trust: OnceLock<Option<Arc<VersionTrustMetadata>>>,
}

/// Where a version's JSON fragment lives until it is hydrated.
#[derive(Debug, Clone)]
enum FragmentSource {
    /// Raw JSON fragment as served by the registry (the serde parse
    /// of a packument body captures these).
    Raw(Arc<RawValue>),
    /// Byte span inside an indexed on-disk metadata mirror, read on
    /// demand from the *held-open* file — reading per hydration
    /// instead of retaining the mirror body keeps a workspace-scale
    /// packument cache out of resident memory. See
    /// [`PackageVersions::from_file_spans`] for the inode-pinning
    /// contract the held handle provides.
    FileSpan { file: Arc<MirrorFile>, offset: u64, len: u32 },
    /// An indexed mirror's fragment, buffered in memory because its
    /// file could not be held open (see
    /// [`PackageVersions::from_buffered_mirror_fragments`]).
    BufferedMirror(Arc<RawValue>),
    /// No fragment — the slot was constructed from an already-typed
    /// manifest (tests, the publish-date filter's slot moves).
    None,
}

impl FragmentSource {
    /// The fragment's JSON text: borrowed for a buffered fragment,
    /// read from the mirror file for [`FragmentSource::FileSpan`],
    /// absent for [`FragmentSource::None`] or unreadable spans.
    fn json(&self) -> Option<Cow<'_, str>> {
        match self {
            FragmentSource::Raw(raw) | FragmentSource::BufferedMirror(raw) => {
                Some(Cow::Borrowed(raw.get()))
            }
            FragmentSource::FileSpan { file, offset, len } => {
                let mut bytes = vec![0u8; *len as usize];
                if let Err(error) = read_exact_at(&file.file, &mut bytes, *offset) {
                    tracing::warn!(
                        target: "pnpm_registry",
                        %error,
                        offset,
                        "could not read a metadata mirror fragment",
                    );
                    return None;
                }
                match String::from_utf8(bytes) {
                    Ok(json) => Some(Cow::Owned(json)),
                    Err(error) => {
                        tracing::warn!(
                            target: "pnpm_registry",
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

    /// Whether the fragment comes from an indexed on-disk mirror,
    /// where a decode failure means a damaged file rather than a
    /// version the registry served badly.
    fn is_mirror_span(&self) -> bool {
        matches!(self, FragmentSource::FileSpan { .. } | FragmentSource::BufferedMirror(_))
    }
}

impl Clone for VersionSlot {
    fn clone(&self) -> Self {
        VersionSlot {
            source: self.source.clone(),
            deprecated: self.deprecated.clone(),
            trust: self.trust.clone(),
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
            deprecated: OnceLock::new(),
            trust: OnceLock::new(),
            parsed: OnceLock::from(Some(Arc::new(manifest))),
        }
    }

    fn hydrate(
        &self,
        version: &str,
        corrupt_mirror_fragment: &AtomicBool,
    ) -> Option<Arc<PackageVersion>> {
        self.parsed
            .get_or_init(|| self.decode(version, corrupt_mirror_fragment).map(Arc::new))
            .clone()
    }

    /// [`VersionPolicyFields`] of the hydrated manifest when there is one,
    /// else of a fresh decode that the slot does not keep.
    fn policy_fields(
        &self,
        version: &str,
        corrupt_mirror_fragment: &AtomicBool,
    ) -> Option<VersionPolicyFields> {
        match self.parsed.get() {
            Some(parsed) => parsed.as_deref().map(VersionPolicyFields::from),
            None => self
                .decode::<PolicyFieldsProbe>(version, corrupt_mirror_fragment)
                .map(VersionPolicyFields::from),
        }
    }

    /// Decode the fragment as `Manifest`, which must fail exactly where a
    /// [`PackageVersion`] does.
    fn decode<Manifest: DeserializeOwned>(
        &self,
        version: &str,
        corrupt_mirror_fragment: &AtomicBool,
    ) -> Option<Manifest> {
        let Some(json) = self.source.json() else {
            self.report_undecodable(version, corrupt_mirror_fragment);
            return None;
        };
        match serde_json::from_str::<Manifest>(&json) {
            Ok(parsed) => Some(parsed),
            Err(error) => {
                tracing::warn!(
                    target: "pnpm_registry",
                    %error,
                    version,
                    "skipping registry version with an undecodable manifest",
                );
                self.report_decode_error(version, &json, corrupt_mirror_fragment);
                None
            }
        }
    }

    /// Only bytes that are not JSON at all mean a damaged mirror. A
    /// well-formed fragment of the wrong shape is how the registry served
    /// that version, and the mirror stores it verbatim, so it stays an
    /// absent version as it is for a fragment read off the network.
    fn report_decode_error(&self, version: &str, json: &str, corrupt_mirror_fragment: &AtomicBool) {
        if serde_json::from_str::<serde::de::IgnoredAny>(json).is_err() {
            self.report_undecodable(version, corrupt_mirror_fragment);
        }
    }

    fn report_undecodable(&self, version: &str, corrupt_mirror_fragment: &AtomicBool) {
        if !self.source.is_mirror_span() {
            return;
        }
        tracing::debug!(
            target: "pnpm_registry",
            version,
            "metadata mirror fragment is damaged; the mirror will be treated as unreadable",
        );
        corrupt_mirror_fragment.store(true, Ordering::Relaxed);
    }
}

impl PackageVersions {
    /// Typed manifest for `version`, hydrating the raw fragment on
    /// first access. `None` when the version is absent *or* its
    /// fragment fails to decode.
    #[must_use]
    pub fn get(&self, version: &str) -> Option<Arc<PackageVersion>> {
        self.slot(version)?.hydrate(version, &self.corrupt_mirror_fragment)
    }

    /// Whether hydrating any version so far read a damaged fragment of
    /// an indexed on-disk mirror. Lazy, like the hydration it reports
    /// on: a corrupt fragment nobody touched goes unnoticed, exactly as
    /// its version going unpicked means nothing was resolved from it.
    #[must_use]
    pub fn has_corrupt_mirror_fragment(&self) -> bool {
        self.corrupt_mirror_fragment.load(Ordering::Relaxed)
    }

    /// Reads the fields used by trust-downgrade checks without hydrating
    /// the rest of a historical version manifest. Successful lookups and
    /// decode failures are cached per version.
    #[must_use]
    pub fn trust_metadata(&self, version: &str) -> Option<&VersionTrustMetadata> {
        let slot = self.slot(version)?;
        slot.trust
            .get_or_init(|| {
                if let Some(Some(parsed)) = slot.parsed.get() {
                    return Some(Arc::new(VersionTrustMetadata::from(parsed.as_ref())));
                }
                let Some(json) = slot.source.json() else {
                    slot.report_undecodable(version, &self.corrupt_mirror_fragment);
                    return None;
                };
                match serde_json::from_str::<VersionTrustMetadata>(&json) {
                    Ok(trust) => Some(Arc::new(trust)),
                    Err(error) => {
                        tracing::warn!(
                            target: "pnpm_registry",
                            %error,
                            version,
                            "skipping registry version with undecodable trust metadata",
                        );
                        slot.report_decode_error(version, &json, &self.corrupt_mirror_fragment);
                        None
                    }
                }
            })
            .as_deref()
    }

    /// Whether `version`'s typed manifest is held, hydrated by [`Self::get`]
    /// or [`Self::iter`]. Never hydrates.
    #[must_use]
    pub fn is_hydrated(&self, version: &str) -> bool {
        self.slot(version)
            .is_some_and(|slot| slot.parsed.get().is_some())
    }

    /// Whether the packument lists `version`. Never hydrates.
    #[must_use]
    pub fn contains_key(&self, version: &str) -> bool {
        self.slot(version).is_some()
    }

    /// Why `version`'s fragment failed to decode, or `None` when the
    /// version is absent or decodes fine.
    ///
    /// [`Self::get`] answers "absent" for both a version the packument
    /// never listed and one whose manifest pnpm couldn't parse. The two
    /// need different reporting — the second is a registry serving a
    /// field in a shape pnpm doesn't model, and the caller can only say
    /// so if it can recover the parse error. Re-parses the fragment, so
    /// this belongs on error paths only.
    #[must_use]
    pub fn decode_error(&self, version: &str) -> Option<String> {
        let slot = self.slot(version)?;
        let json = slot.source.json()?;
        serde_json::from_str::<PackageVersion>(&json).err().map(|error| error.to_string())
    }

    /// Whether `version` is marked deprecated, equivalent to
    /// `get(version).is_some_and(|manifest| manifest.deprecated.is_some())`
    /// but without hydrating the full manifest: an unhydrated fragment
    /// is probed with a single-field deserialize (skipped entirely when
    /// the fragment text doesn't contain the `"deprecated"` key).
    ///
    /// The pick paths consult deprecation for *many* candidate
    /// versions per packument (dist-tag repopulation, the
    /// deprecated-pick fallback); hydrating each candidate parses its
    /// whole manifest — including the flattened catch-all map — and
    /// dominated warm-resolve CPU.
    #[must_use]
    pub fn is_deprecated(&self, version: &str) -> bool {
        let Some(slot) = self.slot(version) else { return false };
        if let Some(parsed) = slot.parsed.get() {
            return parsed.as_ref().is_some_and(|manifest| manifest.deprecated.is_some());
        }
        if let Some(deprecated) = slot.deprecated.get() {
            return *deprecated;
        }
        let Some(json) = slot.source.json() else {
            slot.report_undecodable(version, &self.corrupt_mirror_fragment);
            return false;
        };
        *slot.deprecated.get_or_init(|| {
            if !json.contains(r#""deprecated""#) {
                return false;
            }
            let Ok(probe) = serde_json::from_str::<DeprecatedProbe>(&json) else {
                slot.report_decode_error(version, &json, &self.corrupt_mirror_fragment);
                return false;
            };
            probe.deprecated.is_some()
        })
    }

    /// Version strings in lexical order. Never hydrates.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.slots.iter().map(|(version, _)| version)
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
        self.slots
            .iter()
            .filter_map(|(version, slot)| {
                Some((version, slot.hydrate(version, &self.corrupt_mirror_fragment)?))
            })
    }

    /// The [`VersionPolicyFields`] of every version that decodes as a
    /// [`PackageVersion`], without filling the hydration cache: a version
    /// not hydrated yet is decoded for the caller and kept nowhere else, and
    /// none of its other fields are built. A full walk over a packument that
    /// outlives it, such as one in the resolver's shared cache, would
    /// otherwise keep every manifest the packument lists in memory.
    pub fn iter_policy_fields(&self) -> impl Iterator<Item = (&String, VersionPolicyFields)> {
        self.slots
            .iter()
            .filter_map(|(version, slot)| {
                Some((version, slot.policy_fields(version, &self.corrupt_mirror_fragment)?))
            })
    }

    /// Filtered copy keeping only the versions `keep` accepts. Slots
    /// move as fragments — nothing hydrates. Used by the
    /// publish-date filter, which decides on the packument's `time`
    /// map rather than the manifests.
    #[must_use]
    pub fn filtered(&self, mut keep: impl FnMut(&str) -> bool) -> PackageVersions {
        PackageVersions {
            slots: self.slots
                .iter()
                .filter(|(version, _)| keep(version))
                .map(|(version, slot)| (version.clone(), slot.clone()))
                .collect(),
            corrupt_mirror_fragment: Arc::clone(&self.corrupt_mirror_fragment),
        }
    }

    fn slot(&self, version: &str) -> Option<&VersionSlot> {
        let index = self.slots
            .binary_search_by(|(candidate, _)| candidate.as_str().cmp(version))
            .ok()?;
        Some(&self.slots[index].1)
    }

    fn from_slots(mut slots: Vec<(String, VersionSlot)>) -> Self {
        if !slots.is_sorted_by(|left, right| left.0 <= right.0) {
            slots.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        }
        PackageVersions { slots, corrupt_mirror_fragment: Arc::default() }
    }
}

impl From<HashMap<String, PackageVersion>> for PackageVersions {
    fn from(versions: HashMap<String, PackageVersion>) -> Self {
        PackageVersions::from_slots(
            versions
                .into_iter()
                .map(|(version, manifest)| (version, VersionSlot::from_parsed(manifest)))
                .collect(),
        )
    }
}

impl FromIterator<(String, PackageVersion)> for PackageVersions {
    fn from_iter<Iter: IntoIterator<Item = (String, PackageVersion)>>(iter: Iter) -> Self {
        iter.into_iter()
            .collect::<HashMap<_, _>>()
            .into()
    }
}

impl<'de> Deserialize<'de> for PackageVersions {
    fn deserialize<Deser: Deserializer<'de>>(deserializer: Deser) -> Result<Self, Deser::Error> {
        let raw_map = HashMap::<String, Box<RawValue>>::deserialize(deserializer)?;
        Ok(PackageVersions::from_slots(
            raw_map
                .into_iter()
                .map(|(version, raw)| {
                    (
                        version,
                        VersionSlot {
                            source: FragmentSource::Raw(Arc::from(raw)),
                            parsed: OnceLock::new(),
                            deprecated: OnceLock::new(),
                            trust: OnceLock::new(),
                        },
                    )
                })
                .collect(),
        ))
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
                            target: "pnpm_registry",
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

mod mirror;

#[cfg(test)]
mod tests;
