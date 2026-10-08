//! The indexed on-disk mirror's side of [`PackageVersions`]: the held-open
//! file its fragment spans are read from, and the constructors and
//! accessors the mirror reader and writer use.

use std::{
    borrow::Cow,
    fs::File,
    sync::{Arc, OnceLock},
};

use serde_json::value::RawValue;

use super::{FragmentSource, PackageVersions, VersionSlot};

/// A mirror file held open for on-demand fragment reads, counted
/// against a caller-supplied cap so a fleet of held handles can never
/// exhaust the process's descriptor budget — a load that would exceed
/// the cap falls back to buffering its fragments instead (see
/// [`MirrorFile::try_hold`]).
#[derive(Debug)]
pub struct MirrorFile {
    pub(super) file: File,
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

/// Fill `buf` from `file` at the absolute `offset`. The handle's read
/// cursor is never consulted, and callers must not rely on where it
/// ends up: the unix implementation leaves it untouched, while the
/// Windows one moves it as a `seek_read` side effect.
#[cfg(unix)]
pub fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
}

#[cfg(target_os = "wasi")]
pub fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    std::os::wasi::fs::FileExt::read_exact_at(file, buf, offset)
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
                            deprecated: OnceLock::new(),
                            trust: OnceLock::new(),
                        },
                    )
                })
                .collect(),
        )
    }

    /// Build a map from fragments already read out of an indexed
    /// mirror. The fallback for a mirror the loader could not keep open
    /// (the held-handle cap in [`MirrorFile::try_hold`] was reached):
    /// the fragments stay buffered in memory like a freshly-fetched
    /// packument's, trading residency for a descriptor, and a decode
    /// failure still counts as a damaged mirror.
    #[must_use]
    pub fn from_buffered_mirror_fragments(
        fragments: impl IntoIterator<Item = (String, Box<RawValue>)>,
    ) -> Self {
        PackageVersions::from_slots(
            fragments
                .into_iter()
                .map(|(version, raw)| {
                    (
                        version,
                        VersionSlot {
                            source: FragmentSource::BufferedMirror(Arc::from(raw)),
                            parsed: OnceLock::new(),
                            deprecated: OnceLock::new(),
                            trust: OnceLock::new(),
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
        self.slots
            .iter()
            .filter_map(|(version, slot)| {
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

impl PackageVersions {
    /// Check that every unhydrated mirror fragment is JSON, recording a
    /// damaged one in [`Self::has_corrupt_mirror_fragment`], which this
    /// returns. Unlike a full [`Self::iter`] walk, it keeps no hydrated
    /// manifest, so a caller that reads only part of each version does not
    /// hold the whole packument in memory.
    #[must_use]
    pub fn check_mirror_fragments(&self) -> bool {
        for (version, slot) in &self.slots {
            if !slot.source.is_mirror_span() || slot.parsed.get().is_some() {
                continue;
            }
            match slot.source.json() {
                Some(json) => {
                    if serde_json::from_str::<serde::de::IgnoredAny>(&json).is_err() {
                        slot.report_undecodable(version, &self.corrupt_mirror_fragment);
                    }
                }
                None => slot.report_undecodable(version, &self.corrupt_mirror_fragment),
            }
        }
        self.has_corrupt_mirror_fragment()
    }
}
