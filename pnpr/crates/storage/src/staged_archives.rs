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
//! its bytes, with a small record beside it saying when it was last
//! uploaded. An archive no commit claims expires [`STAGED_ARCHIVE_MAX_AGE`]
//! after its last upload, and a sweep removes it.

use super::{DocumentWrite, RegistryError, Result, Storage};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Reserved namespace of the hosted store holding staged archives.
const STAGED_ARCHIVES_DIR: &str = ".staged-archives";
/// The suffix of the record kept beside each staged archive.
const RECORD_SUFFIX: &str = ".json";

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

/// What is recorded beside a staged archive.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StagedArchiveRecord {
    /// When the archive was last uploaded, in seconds since the Unix epoch.
    staged_at: u64,
    size: u64,
}

impl StagedArchiveRecord {
    fn age(&self, now: u64) -> Duration {
        Duration::from_secs(now.saturating_sub(self.staged_at))
    }

    fn is_claimable(&self, now: u64) -> bool {
        self.age(now) < STAGED_ARCHIVE_MAX_AGE
    }

    fn is_collectable(&self, now: u64) -> bool {
        self.age(now) >= STAGED_ARCHIVE_MAX_AGE + SWEEP_GRACE
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
    /// The namespace holding every archive of this key's principal. The
    /// principal is hashed, so any username is a safe path segment and two
    /// usernames never share one.
    fn namespace(&self) -> String {
        principal_namespace(self.principal)
    }

    fn archive(&self) -> Result<String> {
        let valid = self.digest.len() == 128
            && self.digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if valid {
            Ok(self.digest.to_string())
        } else {
            Err(RegistryError::BadRequest {
                reason: format!("invalid staged archive digest {:?}", self.digest),
            })
        }
    }

    fn record(&self) -> Result<String> {
        Ok(format!("{}{RECORD_SUFFIX}", self.archive()?))
    }
}

fn principal_namespace(principal: &str) -> String {
    let digest = pnpm_crypto_hash::create_hex_hash(principal);
    format!("{STAGED_ARCHIVES_DIR}/{digest}")
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

impl Storage {
    /// The size of `key`'s archive when it is staged with at least
    /// [`STAGED_ARCHIVE_RENEWAL_MARGIN`] of its life left, so a publisher
    /// uploads only what is missing.
    pub async fn staged_archive_size(&self, key: &StagedArchiveKey<'_>) -> Result<Option<u64>> {
        let now = unix_now();
        let Some(record) = self.read_staged_archive_record(key).await? else {
            return Ok(None);
        };
        if record.age(now) + STAGED_ARCHIVE_RENEWAL_MARGIN >= STAGED_ARCHIVE_MAX_AGE {
            return Ok(None);
        }
        Ok(Some(record.size))
    }

    /// Stage `bytes`, which the caller has verified against `key`'s digest,
    /// for `key`'s principal, and renew the archive when it is staged
    /// already. `Ok(false)` when it was staged already.
    pub async fn stage_archive(&self, key: &StagedArchiveKey<'_>, bytes: Vec<u8>) -> Result<bool> {
        self.stage_archive_at(key, bytes, unix_now()).await
    }

    async fn stage_archive_at(
        &self,
        key: &StagedArchiveKey<'_>,
        bytes: Vec<u8>,
        now: u64,
    ) -> Result<bool> {
        let namespace = key.namespace();
        let record = StagedArchiveRecord { staged_at: now, size: bytes.len() as u64 };
        // The record goes first: an archive is never stored without one, so a
        // sweep can tell an upload in flight from an abandoned archive.
        self.write_staged_archive_record(key, &record).await?;
        self.root.create_record(&namespace, &key.archive()?, &bytes).await
    }

    /// The bytes of `key`'s archive, while it is claimable. `None` when it was
    /// never staged by this principal or has expired.
    pub async fn read_staged_archive(&self, key: &StagedArchiveKey<'_>) -> Result<Option<Vec<u8>>> {
        let Some(record) = self.read_staged_archive_record(key).await? else {
            return Ok(None);
        };
        if !record.is_claimable(unix_now()) {
            return Ok(None);
        }
        self.root.read_record(&key.namespace(), &key.archive()?).await
    }

    /// The bytes `principal` has staged and may still claim. Archives of
    /// `principal` past their expiry are removed on the way, so a principal's
    /// own uploads keep its namespace swept between server restarts.
    pub async fn staged_archive_usage(&self, principal: &str) -> Result<u64> {
        let namespace = principal_namespace(principal);
        let keys = self.root.list_record_keys(&namespace).await?;
        let (usage, _) = self.sweep_staged_namespace(&namespace, keys, unix_now()).await?;
        Ok(usage)
    }

    /// Remove every staged archive past its expiry, and every archive left
    /// without a record, reporting how many went. Runs at startup.
    pub async fn sweep_staged_archives(&self) -> Result<usize> {
        self.sweep_staged_archives_at(unix_now()).await
    }

    async fn sweep_staged_archives_at(&self, now: u64) -> Result<usize> {
        let mut by_principal: HashMap<String, Vec<String>> = HashMap::new();
        for key in self.root.list_record_keys(STAGED_ARCHIVES_DIR).await? {
            let Some((principal, object)) = key.split_once('/') else { continue };
            by_principal
                .entry(principal.to_string())
                .or_default()
                .push(object.to_string());
        }
        let mut removed = 0;
        for (principal, keys) in by_principal {
            let namespace = format!("{STAGED_ARCHIVES_DIR}/{principal}");
            removed += self.sweep_staged_namespace(&namespace, keys, now).await?.1;
        }
        Ok(removed)
    }

    /// Sweep one principal's namespace, whose stored keys are `keys`.
    /// Returns the bytes still claimable and how many archives were removed.
    async fn sweep_staged_namespace(
        &self,
        namespace: &str,
        keys: Vec<String>,
        now: u64,
    ) -> Result<(u64, usize)> {
        let mut records = Vec::new();
        let mut archives = Vec::new();
        for key in keys {
            match key.strip_suffix(RECORD_SUFFIX) {
                Some(archive) => records.push(archive.to_string()),
                None => archives.push(key),
            }
        }
        let mut usage = 0_u64;
        let mut removed = 0;
        for archive in &records {
            let record_key = format!("{archive}{RECORD_SUFFIX}");
            let Some(record) = self.read_record_json(namespace, &record_key).await? else {
                continue;
            };
            if !record.is_collectable(now) {
                if record.is_claimable(now) {
                    usage = usage.saturating_add(record.size);
                }
                continue;
            }
            // The archive first, so a record never outlives the archive it
            // vouches for long enough to look like an upload in flight.
            if self.root.remove_record(namespace, archive).await? {
                removed += 1;
            }
            self.root.remove_record(namespace, &record_key).await?;
        }
        for archive in archives {
            if records.contains(&archive) {
                continue;
            }
            // Listed without its record. An upload writes the record before
            // the archive, so one that started after the listing has a record
            // by now; one without is left over from a sweep that stopped
            // between its two removals.
            let record_key = format!("{archive}{RECORD_SUFFIX}");
            if self.root.read_record(namespace, &record_key).await?.is_some() {
                continue;
            }
            if self.root.remove_record(namespace, &archive).await? {
                removed += 1;
            }
        }
        Ok((usage, removed))
    }

    async fn read_staged_archive_record(
        &self,
        key: &StagedArchiveKey<'_>,
    ) -> Result<Option<StagedArchiveRecord>> {
        self.read_record_json(&key.namespace(), &key.record()?).await
    }

    async fn read_record_json(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<StagedArchiveRecord>> {
        let Some(bytes) = self.root.read_record(namespace, key).await? else {
            return Ok(None);
        };
        // A record that does not parse vouches for nothing; the sweep treats
        // its archive as abandoned.
        Ok(serde_json::from_slice(&bytes).ok())
    }

    /// Write `record` for `key`, replacing the one stored. Two uploads of
    /// the same archive racing each other both renew it, so losing the race
    /// to the other is as good as winning it.
    async fn write_staged_archive_record(
        &self,
        key: &StagedArchiveKey<'_>,
        record: &StagedArchiveRecord,
    ) -> Result<()> {
        let namespace = key.namespace();
        let record_key = key.record()?;
        let bytes = serde_json::to_vec(record)?;
        if self.root.create_record(&namespace, &record_key, &bytes).await? {
            return Ok(());
        }
        if let Some(stored) = self.root.read_record(&namespace, &record_key).await? {
            match self.root.replace_record_if_current(&namespace, &record_key, &stored, &bytes)
                .await?
            {
                DocumentWrite::Written | DocumentWrite::Conflict => return Ok(()),
            }
        }
        // Removed between the two calls, by a sweep: create it again.
        self.root.create_record(&namespace, &record_key, &bytes).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
