//! Reading a published archive for the manifest an installer would select.
//!
//! A registry that verifies a publication reads the archive's own
//! `package.json` to compare it with the metadata submitted beside it. The
//! reader refuses any archive in which that choice is not clear-cut: an entry
//! path that escapes the package root, two entries that an installer would
//! extract to the same manifest (on a case-insensitive file system too), a
//! manifest that is not a regular file, more entries than
//! [`MAX_PUBLICATION_ARCHIVE_ENTRIES`], or file paths longer in total than
//! [`MAX_PUBLICATION_PATH_BYTES`]. The caller bounds the decoded size.

use super::{archive_entry_segments, check_trailer, clean_archive_entry_path};
use derive_more::{Display, Error};
use ssri::{Algorithm, Integrity, IntegrityOpts};
use std::{collections::BTreeSet, io::Read};

/// The most entries a published archive may hold.
pub const MAX_PUBLICATION_ARCHIVE_ENTRIES: u64 = 200_000;

/// The most bytes the file paths of a published archive may add up to. The
/// paths are kept in memory, and a long path costs a tar stream little once
/// compressed.
pub const MAX_PUBLICATION_PATH_BYTES: usize = 32 * 1024 * 1024;

/// The largest `package.json` a published archive may hold.
pub const MAX_PUBLICATION_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

/// Ceiling on PAX and GNU extension headers.
const MAX_METADATA_BYTES: u64 = 64 * 1024;

/// The archive's root `package.json`.
const MANIFEST: &str = "package.json";

/// The archive's root shrinkwrap, which npm installs from in place of
/// resolving the package's dependencies.
const SHRINKWRAP: &str = "npm-shrinkwrap.json";

/// What a published archive holds that its manifest is derived from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationArchive {
    /// The exact bytes of the root `package.json`.
    pub manifest: Vec<u8>,
    /// The SHA-512 digest of [`Self::manifest`].
    pub manifest_integrity: Integrity,
    /// The path of the root `package.json` in the archive, as its entry
    /// names it, such as `package/package.json`.
    pub manifest_path: String,
    /// The SHA-512 digest of the root `npm-shrinkwrap.json`, if there is one.
    pub shrinkwrap_integrity: Option<Integrity>,
    /// The path of the root `npm-shrinkwrap.json` in the archive, as its
    /// entry names it, if there is one.
    pub shrinkwrap_path: Option<String>,
    /// Whether the archive has a root `binding.gyp`.
    pub binding_gyp: bool,
    /// Every regular file, as a package-relative path with `/` separators.
    pub files: BTreeSet<String>,
}

/// Why a published archive cannot be read for its manifest.
#[derive(Debug, Display, Error)]
pub enum PublicationArchiveError {
    #[display("the archive is not a readable tar: {_0}")]
    Unreadable(#[error(not(source))] String),
    #[display("archive entry {_0:?} escapes the package root")]
    UnsafePath(#[error(not(source))] String),
    #[display("archive entry {_0:?} makes the package's {_1} ambiguous")]
    AmbiguousManifest(#[error(not(source))] String, #[error(not(source))] &'static str),
    #[display(
        "the archive's global PAX header sets {_0:?}, which some installers apply to every \
         later entry"
    )]
    GlobalHeaderOverride(#[error(not(source))] String),
    #[display("the archive has no package.json")]
    MissingManifest,
    #[display("the archive holds more than {MAX_PUBLICATION_ARCHIVE_ENTRIES} entries")]
    TooManyEntries,
    #[display("the archive's file paths exceed {MAX_PUBLICATION_PATH_BYTES} bytes in total")]
    OversizedPaths,
    #[display("the archive's package.json exceeds {MAX_PUBLICATION_MANIFEST_BYTES} bytes")]
    OversizedManifest,
}

/// Read `tar`, an uncompressed tar stream, for what its manifest is derived
/// from. Refuses an unsafe entry path, an ambiguous or linked manifest,
/// more than [`MAX_PUBLICATION_ARCHIVE_ENTRIES`] entries, file paths longer
/// in total than [`MAX_PUBLICATION_PATH_BYTES`], and a missing or oversized
/// `package.json`.
pub fn read_publication_archive(
    tar: impl Read,
) -> Result<PublicationArchive, PublicationArchiveError> {
    read_publication_archive_within(tar, MAX_PUBLICATION_PATH_BYTES)
}

fn read_publication_archive_within(
    tar: impl Read,
    path_bytes_limit: usize,
) -> Result<PublicationArchive, PublicationArchiveError> {
    let mut archive = tar::Archive::new(tar);
    archive.set_max_metadata_size(Some(MAX_METADATA_BYTES));
    let mut scan = PublicationScan { path_bytes_left: path_bytes_limit, ..Default::default() };
    let entries = archive.entries().map_err(|err| unreadable(&err))?;
    for (index, entry) in entries.enumerate() {
        if index as u64 >= MAX_PUBLICATION_ARCHIVE_ENTRIES {
            return Err(PublicationArchiveError::TooManyEntries);
        }
        scan.visit(&mut entry.map_err(|err| unreadable(&err))?)?;
    }
    // An installer refuses data after the end-of-archive marker, so the
    // archive is refused here too.
    check_trailer(archive.into_inner())
        .map_err(|err| PublicationArchiveError::Unreadable(err.to_string()))?;
    scan.finish()
}

fn unreadable(error: &std::io::Error) -> PublicationArchiveError {
    PublicationArchiveError::Unreadable(error.to_string())
}

#[derive(Default)]
struct PublicationScan {
    manifest: Option<(Vec<u8>, String)>,
    shrinkwrap: Option<(Integrity, String)>,
    binding_gyp: bool,
    files: BTreeSet<String>,
    /// What the paths kept in `files` may still add up to.
    path_bytes_left: usize,
}

impl PublicationScan {
    fn visit(
        &mut self,
        entry: &mut tar::Entry<'_, impl Read>,
    ) -> Result<(), PublicationArchiveError> {
        let raw = entry
            .path()
            .map_err(|err| unreadable(&err))?
            .to_string_lossy()
            .into_owned();
        let entry_type = entry.header().entry_type();
        if entry_type.is_pax_global_extensions() {
            refuse_global_overrides(entry)?;
        }
        if !entry_type.is_file() {
            return refuse_special_manifest(&raw, entry_type);
        }
        let path = clean_archive_entry_path(&raw)
            .map_err(|_| PublicationArchiveError::UnsafePath(raw.clone()))?;
        match root_document(&path) {
            Some(MANIFEST) => self.read_manifest(entry, &raw)?,
            Some(SHRINKWRAP) => self.read_shrinkwrap(entry, &raw)?,
            Some(other) => return Err(PublicationArchiveError::AmbiguousManifest(raw, other)),
            None => {}
        }
        self.binding_gyp |= path == pnpm_package_manifest::BINDING_GYP;
        self.path_bytes_left = self.path_bytes_left
            .checked_sub(path.len())
            .ok_or(PublicationArchiveError::OversizedPaths)?;
        self.files.insert(path);
        Ok(())
    }

    fn read_manifest(
        &mut self,
        entry: &mut tar::Entry<'_, impl Read>,
        raw: &str,
    ) -> Result<(), PublicationArchiveError> {
        if self.manifest.is_some() {
            return Err(PublicationArchiveError::AmbiguousManifest(raw.to_string(), MANIFEST));
        }
        if entry.size() > MAX_PUBLICATION_MANIFEST_BYTES {
            return Err(PublicationArchiveError::OversizedManifest);
        }
        let mut bytes = Vec::new();
        entry
            .take(MAX_PUBLICATION_MANIFEST_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|err| unreadable(&err))?;
        self.manifest = Some((bytes, raw.to_string()));
        Ok(())
    }

    fn read_shrinkwrap(
        &mut self,
        entry: &mut tar::Entry<'_, impl Read>,
        raw: &str,
    ) -> Result<(), PublicationArchiveError> {
        if self.shrinkwrap.is_some() {
            return Err(PublicationArchiveError::AmbiguousManifest(raw.to_string(), SHRINKWRAP));
        }
        let mut hasher = IntegrityOpts::new().algorithm(Algorithm::Sha512);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let read = entry.read(&mut buf).map_err(|err| unreadable(&err))?;
            if read == 0 {
                break;
            }
            hasher.input(&buf[..read]);
        }
        self.shrinkwrap = Some((hasher.result(), raw.to_string()));
        Ok(())
    }

    fn finish(self) -> Result<PublicationArchive, PublicationArchiveError> {
        let (manifest, manifest_path) =
            self.manifest.ok_or(PublicationArchiveError::MissingManifest)?;
        let (shrinkwrap_integrity, shrinkwrap_path) = self.shrinkwrap.unzip();
        let manifest_integrity = IntegrityOpts::new()
            .algorithm(Algorithm::Sha512)
            .chain(&manifest)
            .result();
        Ok(PublicationArchive {
            manifest,
            manifest_integrity,
            manifest_path,
            shrinkwrap_integrity,
            shrinkwrap_path,
            binding_gyp: self.binding_gyp,
            files: self.files,
        })
    }
}

/// The global PAX keys that change where or what a later entry is. node-tar
/// applies a global `size` and `linkpath` to every later entry, while this
/// scan, like the installer here, ignores global headers. An archive that
/// sets one would read differently to the two.
const GLOBAL_OVERRIDES: &[&str] = &["size", "path", "linkpath"];

/// Refuse a global PAX header that sets any of [`GLOBAL_OVERRIDES`]. Other
/// global keys, such as the `comment` `git archive` writes, are accepted.
fn refuse_global_overrides(
    entry: &mut tar::Entry<'_, impl Read>,
) -> Result<(), PublicationArchiveError> {
    if entry.size() > MAX_METADATA_BYTES {
        return Err(unreadable(&std::io::Error::other(format!(
            "a global PAX header exceeds {MAX_METADATA_BYTES} bytes",
        ))));
    }
    let Some(extensions) = entry.pax_extensions().map_err(|err| unreadable(&err))? else {
        return Ok(());
    };
    for extension in extensions {
        let extension = extension.map_err(|err| unreadable(&err))?;
        let key = extension.key().unwrap_or_default();
        if GLOBAL_OVERRIDES.contains(&key) {
            return Err(PublicationArchiveError::GlobalHeaderOverride(key.to_string()));
        }
    }
    Ok(())
}

/// Which root document `path` names when compared without regard to case, or
/// `None` for any other file. An entry that differs from the document's name
/// only in case still lands on it where the file system ignores case, so it
/// reads as `Some` of the canonical name only when it matches exactly.
fn root_document(path: &str) -> Option<&'static str> {
    [MANIFEST, SHRINKWRAP]
        .into_iter()
        .find(|document| path.eq_ignore_ascii_case(document))
        .map(|document| if path == document { document } else { ambiguous_case(document) })
}

/// A marker for a root document spelled in another case, which
/// [`PublicationScan::visit`] refuses.
fn ambiguous_case(document: &'static str) -> &'static str {
    match document {
        MANIFEST => "package.json (spelled in another case)",
        _ => "npm-shrinkwrap.json (spelled in another case)",
    }
}

/// Refuse a non-regular entry whose path escapes the package root, and a
/// link, device, or other non-regular entry that would land on a root
/// document: an installer reads the manifest through whatever it points to.
fn refuse_special_manifest(
    raw: &str,
    entry_type: tar::EntryType,
) -> Result<(), PublicationArchiveError> {
    if !raw.is_empty() && archive_entry_segments(raw).is_none() {
        return Err(PublicationArchiveError::UnsafePath(raw.to_string()));
    }
    if entry_type.is_dir() || entry_type.is_pax_global_extensions() {
        return Ok(());
    }
    // Only the archive root itself, `.`, fails to clean here.
    let Ok(path) = clean_archive_entry_path(raw) else { return Ok(()) };
    match root_document(&path) {
        Some(document) => {
            Err(PublicationArchiveError::AmbiguousManifest(raw.to_string(), document))
        }
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;
