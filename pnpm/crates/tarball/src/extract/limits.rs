//! The decoded-output budget: how much a tar may make pnpm decode before
//! its hash is known to match.

use super::{Read, TarballError};
use std::{cell::Cell, io};

/// Default of the payload ceiling a [`TarLimits`] starts from.
pub const DEFAULT_MAX_TARBALL_UNPACKED_SIZE: u64 = 4 * 1024 * 1024 * 1024;

/// Ceiling on the tar bytes that are not regular-file payload: entry
/// headers, extension headers, padding, the end-of-archive marker, and the
/// bodies of skipped entries. At about 1 KiB per entry it admits a package of
/// some 250,000 files.
pub(crate) const MAX_NON_PAYLOAD_BYTES: u64 = 256 * 1024 * 1024;

/// What may follow the end-of-archive block: the marker's second block plus
/// one 10 KiB tar record, all zero bytes.
const MAX_TRAILER_BYTES: u64 = 512 + 10 * 1024;

/// The limits one archive is extracted under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TarLimits {
    /// Ceiling on the summed size of regular-file entries.
    max_payload: u64,
    /// The package's `dist.unpackedSize`, which tightens the ceiling of a tar.
    unpacked_size: Option<u64>,
}

impl Default for TarLimits {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_TARBALL_UNPACKED_SIZE)
    }
}

impl TarLimits {
    #[must_use]
    pub const fn new(max_payload: u64) -> Self {
        Self { max_payload, unpacked_size: None }
    }

    /// Record the package's `dist.unpackedSize`, the exact sum of its file
    /// payloads. Only a tar is held to it: a `.tgz` keeps the plain ceiling,
    /// so a registry that under-reports the field breaks no install that
    /// worked before tars existed.
    #[must_use]
    pub fn with_unpacked_size(self, unpacked_size: Option<usize>) -> Self {
        let unpacked_size = unpacked_size.and_then(|size| u64::try_from(size).ok());
        Self { unpacked_size, ..self }
    }

    /// The limits for an archive that turned out to be a plain tar.
    pub(crate) fn for_tar(self) -> Self {
        let max_payload =
            self.unpacked_size.map_or(self.max_payload, |size| size.min(self.max_payload));
        Self { max_payload, unpacked_size: None }
    }
}

/// Which limit an archive overran.
#[derive(Debug, derive_more::Display, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveLimit {
    #[display("its files total more than {_0} bytes")]
    Payload(u64),
    #[display("its headers and skipped entries total more than {_0} bytes")]
    NonPayload(u64),
}

/// The running count of one tar walk, shared between the reader beneath
/// the tar parser and the loop over its entries. A regular-file entry
/// announces its size before its body is read, so every byte the reader
/// passes beyond the announced payload is non-payload.
pub(crate) struct TarBudget {
    limits: TarLimits,
    announced_payload: Cell<u64>,
    read: Cell<u64>,
    exceeded: Cell<Option<ArchiveLimit>>,
}

impl TarBudget {
    pub(crate) fn new(limits: TarLimits) -> Self {
        Self {
            limits,
            announced_payload: Cell::new(0),
            read: Cell::new(0),
            exceeded: Cell::new(None),
        }
    }

    pub(crate) fn announce_file(&self, size: u64) -> Result<(), TarballError> {
        let announced = self.announced_payload.get().saturating_add(size);
        if announced > self.limits.max_payload {
            return Err(self.exceed(ArchiveLimit::Payload(self.limits.max_payload)));
        }
        self.announced_payload.set(announced);
        Ok(())
    }

    /// Check the non-payload limit given how far into the archive a walk got.
    fn check_consumed(&self, consumed: u64) -> Result<(), TarballError> {
        if consumed.saturating_sub(self.announced_payload.get()) > MAX_NON_PAYLOAD_BYTES {
            return Err(self.exceed(ArchiveLimit::NonPayload(MAX_NON_PAYLOAD_BYTES)));
        }
        Ok(())
    }

    /// Check a walk over a buffered archive that stopped `consumed` bytes in:
    /// the non-payload limit, an archive that is not empty, and its trailer.
    pub(crate) fn check_buffered_end(
        &self,
        tar_data: &[u8],
        consumed: u64,
    ) -> Result<(), TarballError> {
        self.check_consumed(consumed)?;
        if consumed == 0 {
            return Err(empty_archive_error());
        }
        let rest = usize::try_from(consumed)
            .ok()
            .and_then(|start| tar_data.get(start..))
            .unwrap_or_default();
        check_trailer(rest)
    }

    /// Fail a walk that read nothing: an empty body is not an archive.
    pub(crate) fn check_not_empty(&self) -> Result<(), TarballError> {
        if self.read.get() == 0 {
            return Err(empty_archive_error());
        }
        Ok(())
    }

    pub(crate) fn reader<Inner: Read>(&self, inner: Inner) -> BudgetedReader<'_, Inner> {
        BudgetedReader { inner, budget: self }
    }

    /// `error`, or the limit violation that caused it. The tar parser wraps
    /// reader errors in its own, so the violation is recorded here rather
    /// than recovered from the error chain.
    pub(crate) fn explain(&self, error: TarballError) -> TarballError {
        match self.exceeded.get() {
            Some(limit) => TarballError::ArchiveLimit(limit),
            None => error,
        }
    }

    fn exceed(&self, limit: ArchiveLimit) -> TarballError {
        self.exceeded.set(Some(limit));
        TarballError::ArchiveLimit(limit)
    }

    fn on_read(&self, len: usize) -> io::Result<()> {
        let read = self.read
            .get()
            .saturating_add(len as u64);
        self.read.set(read);
        if self.check_consumed(read).is_err() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "archive limit exceeded"));
        }
        Ok(())
    }
}

/// The reader a streamed tar is parsed from, counting what it passes
/// against a [`TarBudget`].
pub(crate) struct BudgetedReader<'budget, Inner> {
    inner: Inner,
    budget: &'budget TarBudget,
}

impl<Inner: Read> Read for BudgetedReader<'_, Inner> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = self.inner.read(buf)?;
        self.budget.on_read(len)?;
        Ok(len)
    }
}

fn empty_archive_error() -> TarballError {
    TarballError::ReadTarballEntries(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "the archive is empty",
    ))
}

/// Fail an archive with more than [`MAX_TRAILER_BYTES`] after its first
/// end-of-archive block, or with anything but zero bytes there.
///
/// A read error ends the trailer rather than failing the archive: past the
/// marker it can only come from the layer beneath the tar, such as a `.tgz`
/// whose gzip footer does not check out, which pnpm has never rejected.
pub(crate) fn check_trailer(rest: impl Read) -> Result<(), TarballError> {
    let mut trailer = Vec::new();
    let mut rest = rest.take(MAX_TRAILER_BYTES + 1);
    let mut chunk = [0u8; 4096];
    loop {
        match rest.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(len) => trailer.extend_from_slice(&chunk[..len]),
        }
    }
    if trailer.len() as u64 > MAX_TRAILER_BYTES || trailer.iter().any(|byte| *byte != 0) {
        return Err(TarballError::ReadTarballEntries(io::Error::new(
            io::ErrorKind::InvalidData,
            "the archive has data after its end-of-archive marker",
        )));
    }
    Ok(())
}
