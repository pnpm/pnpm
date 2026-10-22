//! The `maxSockets` cap on concurrent connections to one origin.

use crate::{
    DEFAULT_MAX_SOCKETS,
    priority_semaphore::{Permit, PrioritySemaphore},
};
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

/// How the `maxSockets` configuration maps to a per-origin cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostSocketCap {
    Default,
    Disabled,
    Explicit(NonZeroUsize),
}

/// Per-origin concurrent-connection cap, mirroring undici's `connections`
/// option (the `maxSockets` setting pnpm applies per registry origin).
///
/// When an explicit limit is configured, every origin is capped. When
/// uncapped (the default), direct origins are bounded only by the global
/// concurrency semaphore, while proxied origins share a cap of
/// [`DEFAULT_MAX_SOCKETS`] to avoid exhausting proxy connection backlogs
/// or tripping proxy rate limits.
///
/// Each distinct origin gets its own [`PrioritySemaphore`], minted on first
/// request to that origin, so queued downloads cannot take every slot of the
/// origin ahead of the metadata requests resolution waits on. Acquired
/// *before* the global concurrency semaphore of
/// [`ThrottledClient`](crate::ThrottledClient) so a request waiting on a
/// saturated origin does not hold a global concurrency slot.
#[derive(Debug)]
pub(crate) struct HostSocketLimit {
    cap: HostSocketCap,
    per_origin: Mutex<HashMap<String, Arc<PrioritySemaphore>>>,
}

impl HostSocketLimit {
    pub(crate) fn new(setting: Option<usize>) -> Self {
        let cap = match setting {
            None => HostSocketCap::Default,
            Some(0) => HostSocketCap::Disabled,
            Some(n) => HostSocketCap::Explicit(
                NonZeroUsize::new(n).expect("non-zero value expected for n > 0"),
            ),
        };
        Self { cap, per_origin: Mutex::new(HashMap::new()) }
    }

    /// Acquire a permit for `origin` at `priority`, or `None` when uncapped.
    pub(crate) async fn acquire(
        &self,
        origin: &str,
        is_proxied: bool,
        priority: u64,
    ) -> Option<Permit> {
        let limit_num = match self.cap {
            HostSocketCap::Disabled => return None,
            HostSocketCap::Explicit(max) => max.get(),
            HostSocketCap::Default if is_proxied => DEFAULT_MAX_SOCKETS,
            HostSocketCap::Default => return None,
        };
        let semaphore = {
            let mut map = self.per_origin.lock().expect("host-socket-limit mutex poisoned");
            Arc::clone(
                map.entry(origin.to_string())
                    .or_insert_with(|| Arc::new(PrioritySemaphore::new(limit_num))),
            )
        };
        Some(semaphore.acquire(priority).await)
    }

    #[cfg(test)]
    pub(crate) fn queued_waiters(&self, origin: &str) -> usize {
        self.per_origin
            .lock()
            .expect("host-socket-limit mutex poisoned")
            .get(origin)
            .map_or(0, |slots| slots.queued_waiters())
    }
}
