//! Release archives uploaded by integrity ahead of the publish commit that
//! references them.
//!
//! A publisher uploads each archive on its own request, then sends a commit
//! that names the archives by integrity instead of carrying their bytes. The
//! archives wait here, in a reserved namespace of the hosted store, beside
//! the packages they are about to join. Nothing here is readable through a
//! registry route: an archive becomes visible only when a commit copies it
//! into its package, and only the principal that uploaded it can claim it.
//!
//! Each archive is stored under its principal and the hex SHA-512 digest of
//! its bytes, as `<digest>.<incarnation>`, and the metadata database keeps
//! its record (see [`crate::metadata::StagedRecord`]): when it was last
//! uploaded, and whether its bytes are all there. An upload records its
//! archive as incomplete before the bytes arrive, which reserves them against
//! the principal's limits, and as complete once they are stored; only a
//! complete archive counts as staged. An archive no commit claims expires
//! [`STAGED_ARCHIVE_MAX_AGE`] after its last upload, an upload that never
//! completed after [`INCOMPLETE_MAX_AGE`], and a sweep removes both.
//!
//! A sweep records an expired incarnation `gone` in one conditional
//! transaction, which holds only while the record is still the one it found
//! expired, and only then removes that incarnation's key. An upload marks its
//! bytes complete only while their incarnation is not gone, and otherwise
//! stores them again under the next one. So a renewal either lands before
//! the sweep's transaction, which then leaves the archive alone, or stores
//! bytes the sweep's removal never reaches, on every backend.

mod operations;
mod sweep;

use super::{RegistryError, Result};
use crate::metadata::{StagedEntry, StagedId, StagedRecord, StagedState};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Reserved namespace of the hosted store holding staged archives.
const STAGED_ARCHIVES_DIR: &str = ".staged-archives";

/// How long an archive stays claimable after its last upload: as long as an
/// abandoned image push is kept (see [`crate::upload::UPLOAD_MAX_AGE`]).
pub const STAGED_ARCHIVE_MAX_AGE: Duration = crate::upload::UPLOAD_MAX_AGE;

/// How much of [`STAGED_ARCHIVE_MAX_AGE`] must remain for an archive to
/// count as stored when a publisher asks. One closer to expiry reads as
/// missing, so the publisher uploads it again, which renews it, rather than
/// committing against an archive that expires before the commit reads it.
pub const STAGED_ARCHIVE_RENEWAL_MARGIN: Duration = Duration::from_hours(1);

/// How long past its expiry a sweep leaves an archive in place, so a commit
/// that read the archive's record just before it expired can still read the
/// archive itself.
const SWEEP_GRACE: Duration = Duration::from_hours(1);

/// How long an upload may take to complete. An incomplete record older than
/// this is an upload that stopped, and no longer counts against its
/// principal's limits.
pub const INCOMPLETE_MAX_AGE: Duration = Duration::from_hours(1);

/// How many incarnations one upload stores before it gives up on sweeps
/// fencing each off as it completes.
const STAGE_ATTEMPTS: usize = 8;

/// How long ago `record` was last uploaded.
fn age(record: &StagedRecord, now: u64) -> Duration {
    Duration::from_secs(now.saturating_sub(record.staged_at))
}

fn lifetime(record: &StagedRecord) -> Duration {
    match record.state {
        StagedState::Complete => STAGED_ARCHIVE_MAX_AGE,
        StagedState::Incomplete | StagedState::Gone => INCOMPLETE_MAX_AGE,
    }
}

/// Whether a commit can claim the archive.
fn is_claimable(record: &StagedRecord, now: u64) -> bool {
    record.state == StagedState::Complete && age(record, now) < STAGED_ARCHIVE_MAX_AGE
}

/// Whether the archive counts against its principal's limits.
fn is_held(record: &StagedRecord, now: u64) -> bool {
    record.state != StagedState::Gone && age(record, now) < lifetime(record)
}

/// Whether a sweep removes the archive.
fn is_collectable(record: &StagedRecord, now: u64) -> bool {
    record.state != StagedState::Gone && age(record, now) >= lifetime(record) + SWEEP_GRACE
}

/// The incarnation an upload of bytes recorded as `stored` writes: the
/// current one unless a sweep took it.
fn incarnation_for_upload(stored: Option<&StagedRecord>) -> u64 {
    match stored {
        None => 1,
        Some(record) if record.state == StagedState::Gone => record.incarnation + 1,
        Some(record) => record.incarnation,
    }
}

/// What is staged or reserved.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StagedArchiveUsage {
    pub bytes: u64,
    pub archives: usize,
}

impl StagedArchiveUsage {
    /// Count an archive of `size` bytes, when it is held.
    fn hold(&mut self, size: Option<u64>) {
        if let Some(size) = size {
            self.bytes = self.bytes.saturating_add(size);
            self.archives += 1;
        }
    }
}

/// One principal's staged archive: who uploaded it, and the hex SHA-512 of
/// its bytes.
#[derive(Debug, Clone, Copy)]
pub struct StagedArchiveKey<'a> {
    /// The stable identity that uploaded the archive. Only the same
    /// principal can find it again.
    pub principal: &'a str,
    /// The lowercase hex SHA-512 digest of the archive's bytes.
    pub digest: &'a str,
}

impl StagedArchiveKey<'_> {
    fn id(&self) -> Result<StagedId> {
        if !is_digest(self.digest) {
            return Err(RegistryError::BadRequest {
                reason: format!("invalid staged archive digest {:?}", self.digest),
            });
        }
        Ok(StagedId { principal: self.principal.to_string(), digest: self.digest.to_string() })
    }

    /// The namespace holding every archive of this key's principal.
    fn namespace(&self) -> String {
        principal_namespace(self.principal)
    }
}

/// Whether `text` is a lowercase hex SHA-512 digest.
fn is_digest(text: &str) -> bool {
    text.len() == 128
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The key the bytes of `digest` are stored under at `incarnation`.
fn object(digest: &str, incarnation: u64) -> String {
    format!("{digest}.{incarnation}")
}

/// The digest and incarnation `name` is the key of, `None` for a name that
/// is no archive's key, which a sweep never touches.
fn parse_object(name: &str) -> Option<(&str, u64)> {
    let (digest, incarnation) = name.rsplit_once('.')?;
    let canonical =
        !incarnation.starts_with('0') && incarnation.bytes().all(|b| b.is_ascii_digit());
    (is_digest(digest) && canonical).then_some(())?;
    Some((digest, incarnation.parse().ok()?))
}

/// The namespace of `principal`'s archives. The principal is hashed, so any
/// username is a safe path segment and two usernames never share one.
fn principal_namespace(principal: &str) -> String {
    let digest = pnpm_crypto_hash::create_hex_hash(principal);
    format!("{STAGED_ARCHIVES_DIR}/{digest}")
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests;
