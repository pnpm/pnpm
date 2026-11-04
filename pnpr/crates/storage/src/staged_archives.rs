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
//! its bytes, with a small record beside it saying when it was last uploaded
//! and whether its bytes are all there. An upload records its archive as
//! incomplete before the bytes arrive, which reserves them against the
//! principal's limits, and as complete once they are stored; only a complete
//! archive counts as staged. An archive no commit claims expires
//! [`STAGED_ARCHIVE_MAX_AGE`] after its last upload, an upload that never
//! completed after [`INCOMPLETE_MAX_AGE`], and a sweep removes both.

use super::{DocumentWrite, RegistryError, Result, Storage};
use futures_util::future::try_join_all;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
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

/// How long an upload may take to complete. An incomplete record older than
/// this is an upload that stopped, and no longer counts against its
/// principal's limits.
pub const INCOMPLETE_MAX_AGE: Duration = Duration::from_hours(1);

/// How often a record write retries a concurrent writer's change.
const RECORD_WRITE_ATTEMPTS: usize = 8;

/// Records a sweep reads at once.
const SWEEP_CONCURRENCY: usize = 16;

/// What is recorded beside a staged archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StagedArchiveRecord {
    /// When the archive was last uploaded, in seconds since the Unix epoch.
    staged_at: u64,
    size: u64,
    /// Whether the archive's bytes are stored. An upload in flight, or one
    /// that stopped, has reserved its size without storing it.
    complete: bool,
}

impl StagedArchiveRecord {
    fn age(&self, now: u64) -> Duration {
        Duration::from_secs(now.saturating_sub(self.staged_at))
    }

    fn lifetime(&self) -> Duration {
        if self.complete { STAGED_ARCHIVE_MAX_AGE } else { INCOMPLETE_MAX_AGE }
    }

    /// Whether a commit can claim the archive.
    fn is_claimable(&self, now: u64) -> bool {
        self.complete && self.age(now) < STAGED_ARCHIVE_MAX_AGE
    }

    /// Whether the archive counts against its principal's limits.
    fn is_held(&self, now: u64) -> bool {
        self.age(now) < self.lifetime()
    }

    fn is_collectable(&self, now: u64) -> bool {
        self.age(now) >= self.lifetime() + SWEEP_GRACE
    }
}

/// What a principal holds staged or reserved.
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
    /// The namespace holding every archive of this key's principal. The
    /// principal is hashed, so any username is a safe path segment and two
    /// usernames never share one.
    fn namespace(&self) -> String {
        principal_namespace(self.principal)
    }

    fn archive(&self) -> Result<String> {
        if is_archive_key(self.digest) {
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

/// Whether `key` names an archive: a lowercase hex SHA-512 digest. Anything
/// else in a namespace, such as the temporary file of a write in flight, is
/// not the sweep's to touch.
fn is_archive_key(key: &str) -> bool {
    key.len() == 128
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The records and the archives among a namespace's `keys`.
fn classify_keys(keys: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut records = Vec::new();
    let mut archives = Vec::new();
    for key in keys {
        match key.strip_suffix(RECORD_SUFFIX) {
            Some(archive) if is_archive_key(archive) => records.push(key),
            None if is_archive_key(&key) => archives.push(key),
            _ => {}
        }
    }
    (records, archives)
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
        let remaining = STAGED_ARCHIVE_MAX_AGE.saturating_sub(record.age(now));
        if !record.is_claimable(now) || remaining <= STAGED_ARCHIVE_RENEWAL_MARGIN {
            return Ok(None);
        }
        Ok(Some(record.size))
    }

    /// Reserve `size` bytes for `key`'s archive before they arrive, so they
    /// count against its principal's limits while the upload runs. Check the
    /// limits with [`Self::staged_archive_usage`] after reserving, and give
    /// the reservation back with [`Self::release_staged_archive`] when the
    /// upload fails. `Ok(false)` when the archive is staged and claimable
    /// already, by a concurrent upload of the same bytes, which a reservation
    /// must not hide.
    pub async fn reserve_staged_archive(
        &self,
        key: &StagedArchiveKey<'_>,
        size: u64,
    ) -> Result<bool> {
        let now = unix_now();
        let record = StagedArchiveRecord { staged_at: now, size, complete: false };
        self.put_staged_archive_record(key, &record, |stored| stored.is_claimable(now)).await
    }

    /// Give back the reservation of an upload that did not complete. The
    /// record is rewritten as long expired, and only while it is still the
    /// incomplete one: a record that completed meanwhile, by a concurrent
    /// upload of the same bytes, is left alone.
    pub async fn release_staged_archive(&self, key: &StagedArchiveKey<'_>) -> Result<()> {
        let (namespace, record_key) = (key.namespace(), key.record()?);
        let Some(stored) = self.root.read_record(&namespace, &record_key).await? else {
            return Ok(());
        };
        let Ok(record) = serde_json::from_slice::<StagedArchiveRecord>(&stored) else {
            return Ok(());
        };
        if record.complete {
            return Ok(());
        }
        let released = serde_json::to_vec(&StagedArchiveRecord { staged_at: 0, ..record })?;
        // A conflict is a writer that changed the record since: theirs stands.
        self.root.replace_record_if_current(&namespace, &record_key, &stored, &released).await?;
        Ok(())
    }

    /// Stage `bytes`, which the caller has verified against `key`'s digest,
    /// for `key`'s principal, and renew the archive when it is staged
    /// already. `Ok(false)` when its bytes were stored already.
    pub async fn stage_archive(&self, key: &StagedArchiveKey<'_>, bytes: Vec<u8>) -> Result<bool> {
        self.stage_archive_at(key, bytes, unix_now()).await
    }

    async fn stage_archive_at(
        &self,
        key: &StagedArchiveKey<'_>,
        bytes: Vec<u8>,
        now: u64,
    ) -> Result<bool> {
        let record =
            StagedArchiveRecord { staged_at: now, size: bytes.len() as u64, complete: true };
        // The bytes first: a record says the archive is complete only once
        // they are stored.
        let created = self.root.create_record(&key.namespace(), &key.archive()?, &bytes).await?;
        self.write_staged_archive_record(key, &record).await?;
        Ok(created)
    }

    /// The size of `key`'s archive while a commit can claim it.
    pub async fn claimable_staged_archive_size(
        &self,
        key: &StagedArchiveKey<'_>,
    ) -> Result<Option<u64>> {
        let record = self.read_staged_archive_record(key).await?;
        Ok(record
            .filter(|record| record.is_claimable(unix_now()))
            .map(|record| record.size))
    }

    /// Retire `key`'s archive once a commit has copied it into its package:
    /// its record is rewritten as long expired, so it stops counting against
    /// its principal and the sweep collects it. Only the record that was read
    /// is retired, so an upload renewing the archive meanwhile keeps it.
    pub async fn retire_staged_archive(&self, key: &StagedArchiveKey<'_>) -> Result<()> {
        let (namespace, record_key) = (key.namespace(), key.record()?);
        let Some(stored) = self.root.read_record(&namespace, &record_key).await? else {
            return Ok(());
        };
        let Ok(record) = serde_json::from_slice::<StagedArchiveRecord>(&stored) else {
            return Ok(());
        };
        let retired = StagedArchiveRecord { staged_at: 0, complete: false, ..record };
        self.root.replace_record_if_current(
            &namespace,
            &record_key,
            &stored,
            &serde_json::to_vec(&retired)?,
        )
        .await?;
        Ok(())
    }

    /// The bytes of `key`'s archive, while it is claimable. `None` when it was
    /// never staged by this principal, has not completed, or has expired.
    pub async fn read_staged_archive(&self, key: &StagedArchiveKey<'_>) -> Result<Option<Vec<u8>>> {
        let Some(record) = self.read_staged_archive_record(key).await? else {
            return Ok(None);
        };
        if !record.is_claimable(unix_now()) {
            return Ok(None);
        }
        self.root.read_record(&key.namespace(), &key.archive()?).await
    }

    /// What `principal` holds staged or reserved. Archives of `principal`
    /// past their expiry are removed on the way, so a principal's own uploads
    /// keep its namespace swept between the periodic sweeps.
    pub async fn staged_archive_usage(&self, principal: &str) -> Result<StagedArchiveUsage> {
        let namespace = principal_namespace(principal);
        let keys = self.root.list_record_keys(&namespace).await?;
        let (usage, _) = self.sweep_staged_namespace(&namespace, keys, unix_now()).await?;
        Ok(usage)
    }

    /// How many archives `principal` has records for, read from the listing
    /// alone, so a principal with too many is refused before anything reads
    /// them.
    pub async fn staged_archive_count(&self, principal: &str) -> Result<usize> {
        let keys = self.root.list_record_keys(&principal_namespace(principal)).await?;
        Ok(keys
            .iter()
            .filter(|key| key.strip_suffix(RECORD_SUFFIX).is_some_and(is_archive_key))
            .count())
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
    /// Returns what the principal still holds and how many archives were
    /// removed.
    async fn sweep_staged_namespace(
        &self,
        namespace: &str,
        keys: Vec<String>,
        now: u64,
    ) -> Result<(StagedArchiveUsage, usize)> {
        let (records, archives) = classify_keys(keys);
        // Several records at once: on an object store each is a round trip,
        // and a principal's own upload waits for this sweep.
        let mut usage = StagedArchiveUsage::default();
        let mut removed = 0;
        for chunk in records.chunks(SWEEP_CONCURRENCY) {
            let sweeps = chunk
                .iter()
                .map(|record_key| self.sweep_recorded_archive(namespace, record_key, now));
            for (held, expired) in try_join_all(sweeps).await? {
                usage.hold(held);
                removed += usize::from(expired);
            }
        }
        let recorded: HashSet<&str> = records
            .iter()
            .filter_map(|record| record.strip_suffix(RECORD_SUFFIX))
            .collect();
        for archive in archives {
            if !recorded.contains(archive.as_str()) {
                removed += usize::from(self.sweep_unrecorded_archive(namespace, &archive).await?);
            }
        }
        Ok((usage, removed))
    }

    /// Remove the archive `record_key` records once it is past its expiry.
    /// Returns its size while it counts against its principal, and whether
    /// it was removed.
    async fn sweep_recorded_archive(
        &self,
        namespace: &str,
        record_key: &str,
        now: u64,
    ) -> Result<(Option<u64>, bool)> {
        let Some(stored) = self.root.read_record(namespace, record_key).await? else {
            return Ok((None, false));
        };
        // A record that does not parse vouches for nothing: it is collected
        // with its archive, as one long expired.
        if let Ok(record) = serde_json::from_slice::<StagedArchiveRecord>(&stored)
            && !record.is_collectable(now)
        {
            return Ok((record.is_held(now).then_some(record.size), false));
        }
        // An upload that renewed the archive since it was read keeps it.
        if self.root.read_record(namespace, record_key).await? != Some(stored) {
            return Ok((None, false));
        }
        let archive = record_key.strip_suffix(RECORD_SUFFIX).unwrap_or(record_key);
        // The archive first, so a record never outlives the archive it vouches
        // for long enough to look like an upload in flight.
        let removed = self.root.remove_record(namespace, archive).await?;
        self.root.remove_record(namespace, record_key).await?;
        Ok((None, removed))
    }

    /// Remove `archive`, listed without its record, unless its record has
    /// appeared since. An upload reserves the record before it stores the
    /// archive, so one that started after the listing has a record by now;
    /// one without is left over from a sweep that stopped between its two
    /// removals.
    async fn sweep_unrecorded_archive(&self, namespace: &str, archive: &str) -> Result<bool> {
        let record_key = format!("{archive}{RECORD_SUFFIX}");
        if self.root.read_record(namespace, &record_key).await?.is_some() {
            return Ok(false);
        }
        self.root.remove_record(namespace, archive).await
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

    /// Write `record` for `key` unless the stored record is one `keep` says
    /// must stand, retrying while concurrent writers change it. `Ok(false)`
    /// when the stored record was kept.
    async fn put_staged_archive_record(
        &self,
        key: &StagedArchiveKey<'_>,
        record: &StagedArchiveRecord,
        keep: impl Fn(&StagedArchiveRecord) -> bool,
    ) -> Result<bool> {
        let (namespace, record_key) = (key.namespace(), key.record()?);
        let bytes = serde_json::to_vec(record)?;
        for _ in 0..RECORD_WRITE_ATTEMPTS {
            if self.root.create_record(&namespace, &record_key, &bytes).await? {
                return Ok(true);
            }
            // Removed since the create, by a sweep or a release: create it again.
            let Some(stored) = self.root.read_record(&namespace, &record_key).await? else {
                continue;
            };
            if serde_json::from_slice(&stored).is_ok_and(|stored| keep(&stored)) {
                return Ok(false);
            }
            let replaced =
                self.root.replace_record_if_current(&namespace, &record_key, &stored, &bytes)
                    .await?;
            if replaced == DocumentWrite::Written {
                return Ok(true);
            }
        }
        Err(RegistryError::DocumentWriteConflict {
            package: format!("staged archive {}", key.digest),
        })
    }

    /// Write `record` for `key`, replacing the one stored, unless a record
    /// at least as new is stored complete already: two uploads of the same
    /// archive both renew it, so losing the race to the other is as good as
    /// winning it.
    async fn write_staged_archive_record(
        &self,
        key: &StagedArchiveKey<'_>,
        record: &StagedArchiveRecord,
    ) -> Result<()> {
        let staged_at = record.staged_at;
        self.put_staged_archive_record(key, record, |stored| {
            stored.complete && stored.staged_at >= staged_at
        })
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
