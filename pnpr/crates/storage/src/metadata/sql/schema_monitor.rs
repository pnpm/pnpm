use super::{
    adapter::{Adapter, ReadSource},
    schema,
};
use pnpr_error::{RegistryError, Result};
use std::{
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

#[derive(Debug)]
pub(super) struct SchemaMonitor {
    started: Instant,
    next_check: AtomicU64,
    checking: AtomicBool,
    refusal: OnceLock<String>,
}

impl Default for SchemaMonitor {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            next_check: AtomicU64::new(0),
            checking: AtomicBool::new(false),
            refusal: OnceLock::new(),
        }
    }
}

impl SchemaMonitor {
    pub(super) fn result(&self) -> Result<()> {
        match self.refusal.get() {
            Some(reason) => Err(RegistryError::StorageUnavailable { reason: reason.clone() }),
            None => Ok(()),
        }
    }

    pub(super) fn checked(&self) {
        self.next_check.store(self.elapsed_millis().saturating_add(1000), Ordering::Release);
    }

    pub(super) fn claim(&self) -> bool {
        if self.elapsed_millis() < self.next_check.load(Ordering::Acquire) {
            return false;
        }
        if self.checking.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err()
        {
            return false;
        }
        if self.elapsed_millis() < self.next_check.load(Ordering::Acquire) {
            self.checking.store(false, Ordering::Release);
            return false;
        }
        true
    }

    pub(super) fn probe(&self, adapter: &dyn Adapter, name: &str) {
        let _probe = Probe(self);
        let result = adapter.read(ReadSource::Primary, &mut |connection| {
            schema::check_current(connection, name)
        });
        match result {
            Err(error) if adapter.is_unavailable(&error) => {
                tracing::warn!(%error, "Could not recheck the database schema; keeping the last supported version");
            }
            Err(error) => {
                let _ = self.refusal.set(error.to_string());
            }
            Ok(_) => {}
        }
    }

    fn elapsed_millis(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    #[cfg(test)]
    pub(super) fn is_checking(&self) -> bool {
        self.checking.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(super) fn expire(&self) {
        self.next_check.store(0, Ordering::Release);
    }
}

struct Probe<'monitor>(&'monitor SchemaMonitor);

impl Drop for Probe<'_> {
    fn drop(&mut self) {
        self.0.checked();
        self.0.checking.store(false, Ordering::Release);
    }
}
