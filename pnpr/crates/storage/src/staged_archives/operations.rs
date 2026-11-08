//! Uploading, claiming, and giving back staged archives.

use super::{
    STAGE_ATTEMPTS, STAGED_ARCHIVE_MAX_AGE, STAGED_ARCHIVE_RENEWAL_MARGIN, StagedArchiveKey,
    incarnation_for_upload, is_claimable, object, unix_now,
};
use crate::{
    RegistryError, Result, Storage,
    metadata::{StagedRecord, StagedState},
};

impl Storage {
    /// The size of `key`'s archive when it is staged with at least
    /// [`STAGED_ARCHIVE_RENEWAL_MARGIN`] of its life left, so a publisher
    /// uploads only what is missing.
    pub async fn staged_archive_size(&self, key: &StagedArchiveKey<'_>) -> Result<Option<u64>> {
        let now = unix_now();
        let Some(record) = self.database.staged_record(key.id()?).await? else {
            return Ok(None);
        };
        let remaining = STAGED_ARCHIVE_MAX_AGE.saturating_sub(super::age(&record, now));
        if !is_claimable(&record, now) || remaining <= STAGED_ARCHIVE_RENEWAL_MARGIN {
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
        let (_, written) = self.database.change_staged_record(key.id()?, move |stored| {
            if stored.is_some_and(|stored| is_claimable(stored, now)) {
                return None;
            }
            let incarnation = incarnation_for_upload(stored);
            Some(StagedRecord { incarnation, staged_at: now, size, state: StagedState::Incomplete })
        })
        .await?;
        Ok(written.is_some())
    }

    /// Give back the reservation of an upload that did not complete, while
    /// it is still incomplete: one a concurrent upload of the same bytes
    /// completed meanwhile stands.
    pub async fn release_staged_archive(&self, key: &StagedArchiveKey<'_>) -> Result<()> {
        self.database.change_staged_record(key.id()?, |stored| {
            let stored = stored.filter(|stored| stored.state == StagedState::Incomplete)?;
            Some(StagedRecord { staged_at: 0, ..*stored })
        })
        .await?;
        Ok(())
    }

    /// Stage `bytes`, which the caller has verified against `key`'s digest,
    /// for `key`'s principal, and renew the archive when it is staged
    /// already. `Ok(false)` when its bytes were stored already.
    pub async fn stage_archive(&self, key: &StagedArchiveKey<'_>, bytes: Vec<u8>) -> Result<bool> {
        self.stage_archive_at(key, bytes, unix_now()).await
    }

    /// [`Self::stage_archive`] as of `now`. Each attempt stores the bytes
    /// under the incarnation the record holds, and marks it complete only
    /// while no sweep took it; a sweep that did makes the next attempt store
    /// them under the next incarnation, which its removal never reaches.
    pub(super) async fn stage_archive_at(
        &self,
        key: &StagedArchiveKey<'_>,
        bytes: Vec<u8>,
        now: u64,
    ) -> Result<bool> {
        let size = bytes.len() as u64;
        for _ in 0..STAGE_ATTEMPTS {
            let incarnation = self.allocate_staged_archive(key, size, now).await?;
            let name = object(key.digest, incarnation);
            let created = self.root.create_record(&key.namespace(), &name, &bytes).await?;
            if self.complete_staged_archive(key, (incarnation, size), now).await? {
                return Ok(created);
            }
        }
        Err(RegistryError::DocumentWriteConflict {
            package: format!("staged archive {}", key.digest),
        })
    }

    /// The incarnation `key`'s bytes are to be stored under: the recorded
    /// one, or the next one, reserved for `size` bytes, once a sweep took it.
    async fn allocate_staged_archive(
        &self,
        key: &StagedArchiveKey<'_>,
        size: u64,
        now: u64,
    ) -> Result<u64> {
        let (stored, written) = self.database.change_staged_record(key.id()?, move |stored| {
            if stored.is_some_and(|stored| stored.state != StagedState::Gone) {
                return None;
            }
            let incarnation = incarnation_for_upload(stored);
            Some(StagedRecord { incarnation, staged_at: now, size, state: StagedState::Incomplete })
        })
        .await?;
        Ok(written.or(stored).map_or(1, |record| record.incarnation))
    }

    /// Record `key`'s bytes stored under `incarnation` as complete as of
    /// `now`, unless a record at least as new is complete already: two
    /// uploads of the same archive both renew it. `false` when a sweep took
    /// the incarnation, whose bytes then are to be stored again.
    async fn complete_staged_archive(
        &self,
        key: &StagedArchiveKey<'_>,
        (incarnation, size): (u64, u64),
        now: u64,
    ) -> Result<bool> {
        let current = move |stored: &StagedRecord| {
            stored.incarnation == incarnation && stored.state != StagedState::Gone
        };
        let (stored, written) = self.database.change_staged_record(key.id()?, move |stored| {
            let stored = stored.filter(|stored| current(stored))?;
            let newer = stored.state == StagedState::Complete && stored.staged_at >= now;
            (!newer).then_some(StagedRecord {
                incarnation,
                staged_at: now,
                size,
                state: StagedState::Complete,
            })
        })
        .await?;
        Ok(written.is_some() || stored.is_some_and(|stored| current(&stored)))
    }

    /// The size of `key`'s archive while a commit can claim it.
    pub async fn claimable_staged_archive_size(
        &self,
        key: &StagedArchiveKey<'_>,
    ) -> Result<Option<u64>> {
        let record = self.database.staged_record(key.id()?).await?;
        Ok(record
            .filter(|record| is_claimable(record, unix_now()))
            .map(|record| record.size))
    }

    /// Retire `key`'s archive once a commit has copied it into its package:
    /// its record is rewritten as long expired, so it stops counting against
    /// its principal and the sweep collects it. Only the record that was read
    /// is retired, so an upload renewing the archive meanwhile keeps it.
    pub async fn retire_staged_archive(&self, key: &StagedArchiveKey<'_>) -> Result<()> {
        let Some(record) = self.database.staged_record(key.id()?).await? else { return Ok(()) };
        self.retire_record(key, record).await
    }

    /// Rewrite `record` of `key` as long expired and incomplete, unless it
    /// changed since it was read.
    pub(super) async fn retire_record(
        &self,
        key: &StagedArchiveKey<'_>,
        record: StagedRecord,
    ) -> Result<()> {
        self.database.change_staged_record(key.id()?, move |stored| {
            (stored == Some(&record) && record.state != StagedState::Gone).then_some(StagedRecord {
                staged_at: 0,
                state: StagedState::Incomplete,
                ..record
            })
        })
        .await?;
        Ok(())
    }

    /// The bytes of `key`'s archive, while it is claimable. `None` when it was
    /// never staged by this principal, has not completed, or has expired.
    /// A claimable record whose bytes are missing, which only an operator
    /// removing them leaves, is retired, so the publisher's next upload
    /// stores the bytes again instead of finding them staged.
    pub async fn read_staged_archive(&self, key: &StagedArchiveKey<'_>) -> Result<Option<Vec<u8>>> {
        let Some(record) = self.database.staged_record(key.id()?).await? else {
            return Ok(None);
        };
        if !is_claimable(&record, unix_now()) {
            return Ok(None);
        }
        let name = object(key.digest, record.incarnation);
        let archive = self.root.read_record(&key.namespace(), &name).await?;
        if archive.is_none() {
            self.retire_record(key, record).await?;
        }
        Ok(archive)
    }
}
