//! Reading a published archive for the manifest an installer would select.
//!
//! A registry that verifies a publication reads the archive's own
//! `package.json` to compare it with the metadata submitted beside it. The
//! reader refuses any archive in which that choice is not clear-cut: an entry
//! path that escapes the package root, two entries that an installer would
//! extract to the same manifest (on a case-insensitive file system too), a
//! manifest that is not a regular file, or more entries than
//! [`MAX_PUBLICATION_ARCHIVE_ENTRIES`]. The caller bounds the decoded size.

use super::clean_archive_entry_path;
use derive_more::{Display, Error};
use ssri::{Algorithm, Integrity, IntegrityOpts};
use std::{collections::BTreeSet, io::Read};

/// The most entries a published archive may hold.
pub const MAX_PUBLICATION_ARCHIVE_ENTRIES: u64 = 200_000;

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
    /// The SHA-512 digest of the root `npm-shrinkwrap.json`, if there is one.
    pub shrinkwrap_integrity: Option<Integrity>,
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
    #[display("the archive has no package.json")]
    MissingManifest,
    #[display("the archive holds more than {MAX_PUBLICATION_ARCHIVE_ENTRIES} entries")]
    TooManyEntries,
    #[display("the archive's package.json exceeds {MAX_PUBLICATION_MANIFEST_BYTES} bytes")]
    OversizedManifest,
}

/// Read `tar`, an uncompressed tar stream, for what its manifest is derived
/// from. Refuses an unsafe entry path, an ambiguous or linked manifest,
/// more than [`MAX_PUBLICATION_ARCHIVE_ENTRIES`] entries, and a missing or
/// oversized `package.json`.
pub fn read_publication_archive(
    tar: impl Read,
) -> Result<PublicationArchive, PublicationArchiveError> {
    let mut archive = tar::Archive::new(tar);
    archive.set_max_metadata_size(Some(MAX_METADATA_BYTES));
    let mut scan = PublicationScan::default();
    let entries = archive.entries().map_err(|err| unreadable(&err))?;
    for (index, entry) in entries.enumerate() {
        if index as u64 >= MAX_PUBLICATION_ARCHIVE_ENTRIES {
            return Err(PublicationArchiveError::TooManyEntries);
        }
        scan.visit(&mut entry.map_err(|err| unreadable(&err))?)?;
    }
    scan.finish()
}

fn unreadable(error: &std::io::Error) -> PublicationArchiveError {
    PublicationArchiveError::Unreadable(error.to_string())
}

#[derive(Default)]
struct PublicationScan {
    manifest: Option<Vec<u8>>,
    shrinkwrap: Option<Integrity>,
    binding_gyp: bool,
    files: BTreeSet<String>,
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
        self.manifest = Some(bytes);
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
        self.shrinkwrap = Some(hasher.result());
        Ok(())
    }

    fn finish(self) -> Result<PublicationArchive, PublicationArchiveError> {
        let manifest = self.manifest.ok_or(PublicationArchiveError::MissingManifest)?;
        let manifest_integrity = IntegrityOpts::new()
            .algorithm(Algorithm::Sha512)
            .chain(&manifest)
            .result();
        Ok(PublicationArchive {
            manifest,
            manifest_integrity,
            shrinkwrap_integrity: self.shrinkwrap,
            binding_gyp: self.binding_gyp,
            files: self.files,
        })
    }
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

/// Refuse a link, device, or other non-regular entry that would land on a root
/// document: an installer reads the manifest through whatever it points to.
fn refuse_special_manifest(
    raw: &str,
    entry_type: tar::EntryType,
) -> Result<(), PublicationArchiveError> {
    if entry_type.is_dir() || entry_type.is_pax_global_extensions() {
        return Ok(());
    }
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
