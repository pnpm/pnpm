//! Per-origin in-flight cap that a fetch timeout lowers to one request.
//!
//! A timeout while other requests to the same origin are still running
//! means the link to that origin cannot carry them all. Later requests to
//! that origin then wait until only one remains, while every other origin
//! keeps the configured `networkConcurrency`. The cap never rises again
//! within the client's lifetime.

use crate::{HostSocketLimit, ThrottledClient};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// The caps a request takes for its socket origin before the global
/// concurrency permit.
#[derive(Debug)]
pub(crate) struct OriginLimits {
    pub(crate) sockets: HostSocketLimit,
    timeouts: OriginGates,
}

impl OriginLimits {
    pub(crate) fn new(sockets: HostSocketLimit) -> Self {
        OriginLimits { sockets, timeouts: OriginGates::default() }
    }

    /// The timeout cap is taken first, so a request waiting on a lowered
    /// origin holds no `maxSockets` slot either.
    pub(crate) async fn acquire(
        &self,
        origin: &str,
        is_proxied: bool,
    ) -> (OriginPermit, Option<OwnedSemaphorePermit>) {
        let origin_permit = self.timeouts.acquire(origin).await;
        (origin_permit, self.sockets.acquire(origin, is_proxied).await)
    }
}

#[derive(Debug, Default)]
pub(crate) struct OriginGates {
    per_origin: Mutex<HashMap<String, Arc<OriginGate>>>,
}

/// Starts with [`Semaphore::MAX_PERMITS`] slots, so it only counts requests
/// until [`OriginGate::downscale_while_peers_active`] shrinks it to one.
#[derive(Debug)]
struct OriginGate {
    slots: Arc<Semaphore>,
    downscaled: AtomicBool,
    /// Slots still to retire as requests granted before the downscale finish.
    /// Always the slot count minus one after a downscale.
    excess: AtomicUsize,
}

/// Counts one request against its origin until dropped.
#[derive(Debug)]
pub(crate) struct OriginPermit {
    gate: Arc<OriginGate>,
    permit: Option<OwnedSemaphorePermit>,
}

impl OriginGates {
    pub(crate) async fn acquire(&self, origin: &str) -> OriginPermit {
        let gate = Arc::clone(
            self.per_origin
                .lock()
                .expect("origin gate lock poisoned")
                .entry(origin.to_string())
                .or_insert_with(|| Arc::new(OriginGate::new())),
        );
        let permit = Arc::clone(&gate.slots)
            .acquire_owned()
            .await
            .expect("origin gate semaphore is never closed");
        OriginPermit { gate, permit: Some(permit) }
    }

    fn get(&self, origin: &str) -> Option<Arc<OriginGate>> {
        self.per_origin
            .lock()
            .expect("origin gate lock poisoned")
            .get(origin)
            .cloned()
    }
}

impl OriginGate {
    fn new() -> Self {
        OriginGate {
            slots: Arc::new(Semaphore::new(Semaphore::MAX_PERMITS)),
            downscaled: AtomicBool::new(false),
            excess: AtomicUsize::new(0),
        }
    }

    fn downscale_while_peers_active(&self) -> bool {
        let in_flight = Semaphore::MAX_PERMITS - self.slots.available_permits();
        if in_flight <= 1 || self.downscaled.swap(true, Ordering::AcqRel) {
            return false;
        }
        // Every slot not held right now is retired at once. The held ones
        // are retired as they are released, all but the last.
        let held = Semaphore::MAX_PERMITS - self.slots.forget_permits(Semaphore::MAX_PERMITS);
        self.excess.store(held.saturating_sub(1), Ordering::Release);
        true
    }
}

impl Drop for OriginPermit {
    fn drop(&mut self) {
        let Some(permit) = self.permit.take() else {
            return;
        };
        let retire = self.gate.excess
            .try_update(Ordering::AcqRel, Ordering::Acquire, |excess| excess.checked_sub(1))
            .is_ok();
        if retire {
            permit.forget();
        }
    }
}

impl ThrottledClient {
    /// Lower the in-flight cap for `url`'s origin to one request when the
    /// caller still holds its permit for `url` and at least one other
    /// request to the same origin is running.
    ///
    /// Requests already granted finish. The retry, and every later request
    /// to that origin, waits until a single one remains. Other origins keep
    /// the configured `networkConcurrency`. A lone timeout, or an origin
    /// that is already at one request, does not change the cap. Behind a
    /// proxy, the origin is the proxy's.
    ///
    /// Returns whether this call performed the downscale.
    pub fn downscale_while_peers_active(&self, url: &str) -> bool {
        let Some((origin, _)) = self.proxy_routing.effective_socket_origin(url) else {
            return false;
        };
        let downscaled = self.origin_limits.timeouts
            .get(&origin)
            .is_some_and(|gate| gate.downscale_while_peers_active());
        if downscaled {
            tracing::warn!(
                target: "pnpm_network::retry",
                %origin,
                "Fetch timed out while other requests to the same origin were still in flight; lowering its concurrency to 1",
            );
        }
        downscaled
    }

    /// [`Self::downscale_while_peers_active`] when `error` is a timeout.
    /// `url` is the one the failed request's guard was acquired for.
    pub fn downscale_on_timeout(&self, url: &str, error: &reqwest::Error) {
        if error.is_timeout() {
            self.downscale_while_peers_active(url);
        }
    }

    /// Whether [`Self::downscale_while_peers_active`] lowered `url`'s origin.
    #[cfg(test)]
    pub(crate) fn is_origin_downscaled(&self, url: &str) -> bool {
        self.proxy_routing
            .effective_socket_origin(url)
            .and_then(|(origin, _)| self.origin_limits.timeouts.get(&origin))
            .is_some_and(|gate| gate.downscaled.load(Ordering::Acquire))
    }
}

#[cfg(test)]
mod tests;
