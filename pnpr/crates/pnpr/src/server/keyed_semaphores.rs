use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Per-key concurrency limiter whose idle entries evict themselves.
///
/// Each key with at least one holder or waiter owns a [`Semaphore`] of
/// `permits_per_key` permits. Once the last holder or waiter for a key drops
/// (including when an `acquire` future is cancelled before receiving its
/// permit), its map entry is removed so distinct callers over time cannot
/// accumulate entries.
#[derive(Clone)]
pub(crate) struct KeyedSemaphores {
    entries: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    permits_per_key: usize,
}

impl KeyedSemaphores {
    pub(crate) fn new(permits_per_key: usize) -> Self {
        assert!(permits_per_key > 0, "permits_per_key must be non-zero");
        Self { entries: Arc::new(Mutex::new(HashMap::new())), permits_per_key }
    }

    pub(crate) fn try_acquire(&self, key: &str) -> Option<KeyedPermit> {
        let mut entries = self.entries.lock().expect("keyed semaphore map is never poisoned");
        let semaphore = Arc::clone(
            entries
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(self.permits_per_key))),
        );
        let Ok(permit) = semaphore.try_acquire_owned() else {
            if entries
                .get(key)
                .is_some_and(|entry| Arc::strong_count(entry) == 1)
            {
                entries.remove(key);
            }
            return None;
        };
        drop(entries);
        Some(KeyedPermit {
            entries: Arc::clone(&self.entries),
            key: key.to_string(),
            permit: Some(permit),
        })
    }

    pub(crate) async fn acquire(&self, key: &str) -> KeyedPermit {
        let semaphore = Arc::clone(
            self.entries
                .lock()
                .expect("keyed semaphore map is never poisoned")
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(self.permits_per_key))),
        );
        // Built before `.await` so cancelling `acquire_owned` drops the
        // `AcquireOwned` future's `Arc<Semaphore>` first and runs `Drop` on
        // `guard` second, evicting the map entry if no other task shares it.
        let mut guard =
            KeyedPermit { entries: Arc::clone(&self.entries), key: key.to_string(), permit: None };
        let permit = semaphore.acquire_owned().await.expect("a keyed semaphore is never closed");
        guard.permit = Some(permit);
        guard
    }

    #[cfg(test)]
    pub(crate) fn tracked_keys(&self) -> usize {
        self.entries
            .lock()
            .expect("keyed semaphore map is never poisoned")
            .len()
    }
}

pub(crate) struct KeyedPermit {
    entries: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    key: String,
    permit: Option<OwnedSemaphorePermit>,
}

impl Drop for KeyedPermit {
    fn drop(&mut self) {
        drop(self.permit.take());
        let mut entries = self.entries.lock().expect("keyed semaphore map is never poisoned");
        if entries
            .get(&self.key)
            .is_some_and(|semaphore| Arc::strong_count(semaphore) == 1)
        {
            entries.remove(&self.key);
        }
    }
}

#[cfg(test)]
mod tests;
