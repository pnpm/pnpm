//! Per-origin in-flight cap that a fetch timeout lowers to one request.
//!
//! A timeout while other requests to the same origin are still running
//! means the link to that origin cannot carry them all. Later requests to
//! that origin then wait until only one remains, while every other origin
//! keeps the configured `networkConcurrency`. The cap never rises again
//! within the client's lifetime.

use crate::{
    HostSocketLimit, ThrottledClient, priority_semaphore::Permit, registry_limit::RegistryLimits,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// The caps a request takes for its origin before the global concurrency
/// permit.
#[derive(Debug)]
pub(crate) struct OriginLimits {
    registries: RegistryLimits,
    pub(crate) sockets: HostSocketLimit,
    timeouts: OriginGates,
}

impl OriginLimits {
    pub(crate) fn new(registries: RegistryLimits, sockets: HostSocketLimit) -> Self {
        OriginLimits { registries, sockets, timeouts: OriginGates::default() }
    }

    #[cfg(test)]
    pub(crate) fn registries(&self) -> &RegistryLimits {
        &self.registries
    }

    /// The registry cap is taken first, then the timeout gate, then the
    /// `maxSockets` slot, so a request waiting on one holds none of the later
    /// ones. The registry cap is keyed by `url`'s own origin, the others by
    /// `origin`, the connection's.
    pub(crate) async fn acquire(
        &self,
        url: &str,
        origin: &str,
        is_proxied: bool,
        priority: u64,
    ) -> (OriginPermit, Option<Permit>) {
        let registry_slot = self.registries.acquire(url, priority).await;
        let mut origin_permit = self.timeouts.acquire(origin).await;
        origin_permit.registry_slot = registry_slot;
        (origin_permit, self.sockets.acquire(origin, is_proxied, priority).await)
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
    /// Requests past every admission step, so on the network. Slots also
    /// cover requests still queued for the global semaphore, which are not
    /// peers of a timed-out request.
    active: AtomicUsize,
    /// Held across a downscale and across every permit release, so no slot
    /// can return to the pool between retiring the free slots and recording
    /// how many held ones still have to go.
    retirement: Mutex<Retirement>,
}

#[derive(Debug, Default)]
struct Retirement {
    downscaled: bool,
    /// Slots still to retire as requests granted before the downscale finish.
    /// Always the slot count minus one after a downscale.
    excess: usize,
}

/// Counts one request against its origin until dropped.
#[derive(Debug)]
pub(crate) struct OriginPermit {
    gate: Arc<OriginGate>,
    permit: Option<OwnedSemaphorePermit>,
    active: bool,
    /// The slot of the `registries` entry capping this request's origin.
    registry_slot: Option<Permit>,
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
        OriginPermit { gate, permit: Some(permit), active: false, registry_slot: None }
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
            active: AtomicUsize::new(0),
            retirement: Mutex::default(),
        }
    }

    fn retirement(&self) -> MutexGuard<'_, Retirement> {
        // A release runs in `Drop`, where a poisoned-lock panic would abort.
        self.retirement.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn downscale_while_peers_active(&self) -> bool {
        let mut retirement = self.retirement();
        if retirement.downscaled || self.active.load(Ordering::Acquire) <= 1 {
            return false;
        }
        // Every slot not held right now is retired at once. The held ones
        // are retired as they are released, all but the last.
        let held = Semaphore::MAX_PERMITS - self.slots.forget_permits(Semaphore::MAX_PERMITS);
        *retirement = Retirement { downscaled: true, excess: held.saturating_sub(1) };
        true
    }
}

impl OriginPermit {
    /// Count the request as on the network, once it holds every other permit.
    pub(crate) fn mark_active(&mut self) {
        if !self.active {
            self.active = true;
            self.gate.active.fetch_add(1, Ordering::AcqRel);
        }
    }
}

impl Drop for OriginPermit {
    fn drop(&mut self) {
        if self.active {
            self.gate.active.fetch_sub(1, Ordering::AcqRel);
        }
        let Some(permit) = self.permit.take() else {
            return;
        };
        let mut retirement = self.gate.retirement();
        if retirement.excess > 0 {
            retirement.excess -= 1;
            permit.forget();
        } else {
            drop(permit);
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
            .is_some_and(|gate| gate.retirement().downscaled)
    }
}

#[cfg(test)]
mod tests;
