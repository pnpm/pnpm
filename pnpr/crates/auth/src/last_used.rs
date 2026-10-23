use super::{TokenBackend, unix_seconds};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// How far a token's stored last-used time may lag behind its actual last
/// use, in seconds. A token in steady use costs one write per interval, not
/// one per request.
pub const LAST_USED_GRANULARITY_SECS: u64 = 60;

/// The smallest claim count that triggers a prune. Below it the map is
/// cheaper to keep than to scan.
const MIN_CLAIMS_BEFORE_PRUNE: usize = 1024;

/// Records when tokens are used. The write to the token store happens off
/// the request path, at most once per [`LAST_USED_GRANULARITY_SECS`] per
/// token. A request never waits for it or learns that it failed: a lost
/// write leaves the stored time at most one interval stale, and the next
/// request past the interval writes again.
pub struct LastUsedWriter {
    tokens: Arc<dyn TokenBackend>,
    claims: Mutex<Claims>,
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
            claims: Mutex::new(Claims {
                written_at: HashMap::new(),
                prune_at: MIN_CLAIMS_BEFORE_PRUNE,
            }),
        }
    }

    /// Record that the token with `key` was used now. `last_used_at` is the
    /// time the store holds for it, which the lookup that authenticated the
    /// request already read.
    pub fn record(&self, key: &str, last_used_at: u64) {
        let now = unix_seconds();
        if !self.claim(key, last_used_at, now) {
            return;
        }
        let tokens = Arc::clone(&self.tokens);
        let key = key.to_owned();
        tokio::spawn(async move {
            if let Err(err) = tokens.touch_key(&key, now).await {
                tracing::debug!(error = %err, "could not record a token's last use");
            }
        });
    }

    /// Whether a write for `key` is due at `now`, claiming the interval for
    /// the caller when it is.
    fn claim(&self, key: &str, last_used_at: u64, now: u64) -> bool {
        if within_interval(last_used_at, now) {
            return false;
        }
        let mut claims = self.claims.lock().expect("LastUsedWriter mutex poisoned");
        if claims.written_at.get(key).is_some_and(|&written_at| within_interval(written_at, now)) {
            return false;
        }
        claims.insert(key, now);
        true
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
