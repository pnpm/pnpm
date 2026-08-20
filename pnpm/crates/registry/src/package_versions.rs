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
//! A fragment that fails to decode behaves as if the version were
//! absent from the packument (with a `tracing::warn`), mirroring the
//! tolerance of JavaScript package managers, which never validate
//! version entries they don't pick.

use std::{
    borrow::Cow,
    collections::HashMap,
    fs::File,
    sync::{Arc, OnceLock},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::package_version::{PackageVersion, deserialize_deprecated_field};

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
}

#[derive(Debug)]
struct VersionSlot {
    source: FragmentSource,
    /// Hydration cache. `Some(None)` records a fragment that failed
    /// to decode so the parse error is paid (and warned about) once.
    parsed: OnceLock<Option<Arc<PackageVersion>>>,
}

/// A mirror file held open for on-demand fragment reads, counted
/// against a caller-supplied cap so a fleet of held handles can never
/// exhaust the process's descriptor budget — a load that would exceed
/// the cap falls back to buffering its fragments instead (see
/// [`MirrorFile::try_hold`]).
#[derive(Debug)]
pub struct MirrorFile {
    file: File,
}

static HELD_MIRROR_FILES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl MirrorFile {
    /// Wrap `file` for span reads when fewer than `cap` mirror files
    /// are currently held; hand the file back otherwise so the caller
    /// can buffer its contents and close it.
    pub fn try_hold(file: File, cap: usize) -> Result<Arc<MirrorFile>, File> {
        let mut held = HELD_MIRROR_FILES.load(std::sync::atomic::Ordering::Relaxed);
        loop {
            if held >= cap {
                return Err(file);
            }
            match HELD_MIRROR_FILES.compare_exchange_weak(
                held,
                held + 1,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(Arc::new(MirrorFile { file })),
                Err(current) => held = current,
            }
        }
    }
}

impl Drop for MirrorFile {
    fn drop(&mut self) {
        HELD_MIRROR_FILES.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
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
    /// No fragment — the slot was constructed from an already-typed
    /// manifest (tests, the publish-date filter's slot moves).
    None,
}

impl FragmentSource {
    /// The fragment's JSON text: borrowed for [`FragmentSource::Raw`],
    /// read from the mirror file for [`FragmentSource::FileSpan`],
    /// absent for [`FragmentSource::None`] or unreadable spans.
    fn json(&self) -> Option<Cow<'_, str>> {
        match self {
            FragmentSource::Raw(raw) => Some(Cow::Borrowed(raw.get())),
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
}

/// Fill `buf` from `file` at the absolute `offset`. The handle's read
/// cursor is never consulted, and callers must not rely on where it
/// ends up: the unix implementation leaves it untouched, while the
/// Windows one moves it as a `seek_read` side effect.
#[cfg(unix)]
pub fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
}

/// See the unix sibling for the shared contract.
#[cfg(windows)]
pub fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        match std::os::windows::fs::FileExt::seek_read(file, buf, offset) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "mirror fragment span reaches past the end of the file",
                ));
            }
            Ok(read) => {
                buf = &mut buf[read..];
                offset += read as u64;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
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
                            target: "pnpm_registry",
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
        self.slot(version)?.hydrate(version)
    }

    /// Whether the packument lists `version`. Never hydrates.
    #[must_use]
    pub fn contains_key(&self, version: &str) -> bool {
        self.slot(version).is_some()
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
        let Some(json) = slot.source.json() else { return false };
        if !json.contains(r#""deprecated""#) {
            return false;
        }
        serde_json::from_str::<DeprecatedProbe>(&json).is_ok_and(|probe| probe.deprecated.is_some())
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

    fn slot(&self, version: &str) -> Option<&VersionSlot> {
        let index =
            self.slots.binary_search_by(|(candidate, _)| candidate.as_str().cmp(version)).ok()?;
        Some(&self.slots[index].1)
    }

    fn from_slots(mut slots: Vec<(String, VersionSlot)>) -> Self {
        if !slots.is_sorted_by(|left, right| left.0 <= right.0) {
            slots.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        }
        PackageVersions { slots }
    }
}

/// Constructors and accessors for the indexed on-disk mirror format
/// (see `pnpm-resolving-npm-resolver`'s `mirror` module, which owns
/// the file layout).
impl PackageVersions {
    /// Build a map whose fragments are byte spans read on demand from
    /// the held-open `file` (the indexed mirror). Nothing parses until
    /// a version hydrates, and no fragment bytes stay resident.
    ///
    /// The handle pins the inode: mirror rewrites go through a temp
    /// file followed by `rename`, so the bytes behind this open handle
    /// can never shift under the recorded spans.
    #[must_use]
    pub fn from_file_spans(
        file: &Arc<MirrorFile>,
        spans: impl IntoIterator<Item = (String, u64, u32)>,
    ) -> Self {
        PackageVersions::from_slots(
            spans
                .into_iter()
                .map(|(version, offset, len)| {
                    (
                        version,
                        VersionSlot {
                            source: FragmentSource::FileSpan {
                                file: Arc::clone(file),
                                offset,
                                len,
                            },
                            parsed: OnceLock::new(),
                        },
                    )
                })
                .collect(),
        )
    }

    /// Build a map from already-extracted raw JSON fragments. The
    /// fallback for a mirror the loader could not keep open (the
    /// held-handle cap in [`MirrorFile::try_hold`] was reached): the
    /// fragments stay buffered in memory like a freshly-fetched
    /// packument's, trading residency for a descriptor.
    #[must_use]
    pub fn from_raw_fragments(
        fragments: impl IntoIterator<Item = (String, Box<RawValue>)>,
    ) -> Self {
        PackageVersions::from_slots(
            fragments
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
        )
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
                            target: "pnpm_registry",
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
        iter.into_iter().collect::<HashMap<_, _>>().into()
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

#[cfg(test)]
mod tests;
