use super::{TokenBackend, sha256_hex, unix_seconds};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::Semaphore;

/// How far a token's stored last-used time may lag behind its actual last
/// use, in seconds. A token in steady use costs one write per interval, not
/// one per request.
pub const LAST_USED_GRANULARITY_SECS: u64 = 60;

/// How many writes may run at once. They share the token store's connection
/// pool with the lookups that authenticate requests, so a burst of distinct
/// tokens must not take the pool from those.
const MAX_WRITES_IN_FLIGHT: usize = 4;

/// The smallest claim count that triggers a prune. Below it the map is
/// cheaper to keep than to scan.
const MIN_CLAIMS_BEFORE_PRUNE: usize = 1024;

/// Records when tokens are used. The write to the token store happens off
/// the request path, at most once per [`LAST_USED_GRANULARITY_SECS`] per
/// token. A request never waits for it or learns that it failed: the
/// token's next request tries again.
pub struct LastUsedWriter {
    tokens: Arc<dyn TokenBackend>,
    claims: Arc<Mutex<Claims>>,
    writes_in_flight: Arc<Semaphore>,
}

/// The tokens whose write for the current interval is already claimed, by
/// key, so concurrent requests from one token write once even before the
/// first write lands or an embedded replica syncs it back. Entries older
/// than an interval are dropped once the map outgrows `prune_at`, which
/// doubles each time so pruning stays amortized.
struct Claims {
    written_at: HashMap<String, u64>,
    prune_at: usize,
}

impl LastUsedWriter {
    #[must_use]
    pub fn new(tokens: Arc<dyn TokenBackend>) -> Self {
        Self {
            tokens,
            claims: Arc::new(Mutex::new(Claims {
                written_at: HashMap::new(),
                prune_at: MIN_CLAIMS_BEFORE_PRUNE,
            })),
            writes_in_flight: Arc::new(Semaphore::new(MAX_WRITES_IN_FLIGHT)),
        }
    }

    /// Record that the token with `key` was used now. `last_used_at` is the
    /// time the store holds for it, which the lookup that authenticated the
    /// request already read.
    pub fn record(&self, key: &str, last_used_at: u64) {
        self.record_with(last_used_at, || key.to_owned());
    }

    /// [`Self::record`] for a raw token. It is hashed to its key only once a
    /// write is due.
    pub fn record_raw(&self, raw: &str, last_used_at: u64) {
        self.record_with(last_used_at, || sha256_hex(raw.as_bytes()));
    }

    /// Whether a write was started.
    fn record_with(&self, last_used_at: u64, key: impl FnOnce() -> String) -> bool {
        let now = unix_seconds();
        if within_interval(last_used_at, now) {
            return false;
        }
        let key = key();
        if !self.claim(&key, now) {
            return false;
        }
        let Ok(permit) = Arc::clone(&self.writes_in_flight).try_acquire_owned() else {
            // The pool is busy with writes; the next request from this token
            // gets to try again.
            release_claim(&self.claims, &key, None);
            return false;
        };
        let tokens = Arc::clone(&self.tokens);
        let claims = Arc::clone(&self.claims);
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(err) = tokens.touch_key(&key, now).await {
                tracing::debug!(error = %err, "could not record a token's last use");
                // Let the token's next request retry, unless a later write
                // has claimed the interval since.
                release_claim(&claims, &key, Some(now));
            }
        });
        true
    }

    /// Whether the interval at `now` is free for `key`, claiming it when so.
    fn claim(&self, key: &str, now: u64) -> bool {
        let mut claims = self.claims.lock().expect("LastUsedWriter mutex poisoned");
        if claims.written_at.get(key).is_some_and(|&written_at| within_interval(written_at, now)) {
            return false;
        }
        claims.insert(key, now);
        true
    }
}

/// Drop the claim on `key`, or with `written_at` given only the claim made
/// at that time.
fn release_claim(claims: &Mutex<Claims>, key: &str, written_at: Option<u64>) {
    let mut claims = claims.lock().expect("LastUsedWriter mutex poisoned");
    if written_at.is_none_or(|at| claims.written_at.get(key) == Some(&at)) {
        claims.written_at.remove(key);
    }
}

impl Claims {
    fn insert(&mut self, key: &str, now: u64) {
        if self.written_at.len() >= self.prune_at {
            self.written_at.retain(|_, &mut written_at| within_interval(written_at, now));
            self.prune_at = (self.written_at.len() * 2).max(MIN_CLAIMS_BEFORE_PRUNE);
        }
        self.written_at.insert(key.to_owned(), now);
    }
}

fn within_interval(written_at: u64, now: u64) -> bool {
    now < written_at.saturating_add(LAST_USED_GRANULARITY_SECS)
}

#[cfg(test)]
mod tests;
