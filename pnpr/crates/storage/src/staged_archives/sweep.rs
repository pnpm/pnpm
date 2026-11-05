//! Counting a principal's staged archives, and sweeping the expired ones.

use super::{
    STAGED_ARCHIVES_DIR, StagedArchiveKey, StagedArchiveUsage, StagedEntry, is_collectable,
    is_held, object, parse_object, principal_namespace, unix_now,
};
use crate::{
    Result, Storage,
    metadata::{StagedId, StagedRecord, StagedState},
};
use std::collections::HashMap;

impl Storage {
    /// What `principal` holds staged or reserved. Archives of `principal`
    /// past their expiry are swept on the way, so a principal's own uploads
    /// keep its archives swept between the periodic sweeps.
    pub async fn staged_archive_usage(&self, principal: &str) -> Result<StagedArchiveUsage> {
        let now = unix_now();
        let entries = self.database.staged_records(Some(principal.to_string())).await?;
        let mut usage = StagedArchiveUsage::default();
        for (id, record) in entries {
            if is_collectable(&record, now) {
                self.sweep_record(&id, record, now).await?;
            } else {
                usage.hold(is_held(&record, now).then_some(record.size));
            }
        }
        Ok(usage)
    }

    /// How many archives `principal` holds: records that have not expired.
    /// Expired ones count for nothing, so reservations that never completed
    /// do not refuse the next upload before a sweep takes them.
    pub async fn staged_archive_count(&self, principal: &str) -> Result<usize> {
        let now = unix_now();
        Ok(self.database
            .staged_records(Some(principal.to_string()))
            .await?
            .iter()
            .filter(|(_, record)| is_held(record, now))
            .count())
    }

    /// Remove every staged archive past its expiry, and every stored
    /// incarnation no record holds, reporting how many went. Runs at
    /// startup.
    pub async fn sweep_staged_archives(&self) -> Result<usize> {
        self.sweep_staged_archives_at(unix_now()).await
    }

    pub(super) async fn sweep_staged_archives_at(&self, now: u64) -> Result<usize> {
        // The keys are listed before the records are read: an upload records
        // its incarnation before it stores the bytes, so every key listed
        // here has its record by the time it is read.
        let stored = self.root.list_record_keys(STAGED_ARCHIVES_DIR).await?;
        let entries = self.database.staged_records(None).await?;
        let mut removed = 0;
        for (id, record) in &entries {
            if is_collectable(record, now) {
                removed += usize::from(self.sweep_record(id, *record, now).await?);
            }
        }
        Ok(removed + self.sweep_unrecorded(stored, &entries).await?)
    }

    /// Record `record` of `id` gone, while it is still the expired record
    /// that was read, and only then remove its incarnation's key. Reports
    /// whether it was taken.
    pub(super) async fn sweep_record(
        &self,
        id: &StagedId,
        record: StagedRecord,
        now: u64,
    ) -> Result<bool> {
        let (_, written) = self.database.change_staged_record(id.clone(), move |stored| {
            (stored == Some(&record) && is_collectable(&record, now)).then_some(StagedRecord {
                state: StagedState::Gone,
                ..record
            })
        })
        .await?;
        if written.is_none() {
            return Ok(false);
        }
        let name = object(&id.digest, record.incarnation);
        self.root.remove_record(&principal_namespace(&id.principal), &name).await?;
        Ok(true)
    }

    /// Remove every key of `stored` no record of `entries` makes the
    /// current incarnation: one below it, or at it once gone. A name that is
    /// no archive's key is left alone.
    async fn sweep_unrecorded(
        &self,
        stored: Vec<String>,
        entries: &[StagedEntry],
    ) -> Result<usize> {
        let current: HashMap<(String, &str), &StagedRecord> = entries
            .iter()
            .map(|(id, record)| ((principal_namespace(&id.principal), id.digest.as_str()), record))
            .collect();
        let mut removed = 0;
        for key in stored {
            let Some((hash, name)) = key.split_once('/') else { continue };
            let Some((digest, incarnation)) = parse_object(name) else { continue };
            let namespace = format!("{STAGED_ARCHIVES_DIR}/{hash}");
            let dead = current
                .get(&(namespace.clone(), digest))
                .is_none_or(|record| is_dead(record, incarnation));
            if dead {
                removed += usize::from(self.root.remove_record(&namespace, name).await?);
            }
        }
        Ok(removed)
    }
}

/// Whether `incarnation` of an archive recorded as `record` can never be
/// claimed again.
fn is_dead(record: &StagedRecord, incarnation: u64) -> bool {
    incarnation < record.incarnation
        || (incarnation == record.incarnation && record.state == StagedState::Gone)
}

impl StagedArchiveKey<'_> {
    #[cfg(test)]
    pub(super) fn object_name(&self, incarnation: u64) -> String {
        object(self.digest, incarnation)
    }
}
