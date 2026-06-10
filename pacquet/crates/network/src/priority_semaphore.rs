//! Class-aware permit dispenser backing
//! [`ThrottledClient`](crate::ThrottledClient).
//!
//! Two request classes share the pool:
//!
//! * **Latency** ([`crate::UNPRIORITIZED`]) — packument and other
//!   metadata fetches that gate resolution progress. Served FIFO.
//! * **Throughput** (any other priority) — tarball downloads, ranked
//!   by their estimated pipeline work so the most expensive archives
//!   claim freed slots first (the longest-processing-time-first
//!   scheduling argument; see
//!   [pnpm/pnpm#12230](https://github.com/pnpm/pnpm/issues/12230) for
//!   the per-connection bandwidth measurements behind it).
//!
//! Neither class may starve the other. Strictly preferring latency
//! work was measured to *serialize* a cold fresh install — during the
//! resolution burst no tarball ever got a slot, so the download
//! pipeline started only after resolution finished, costing the whole
//! resolve/fetch overlap. Instead the throughput class is guaranteed a
//! reserved share of the pool (half, rounded up — but never all of
//! it): when downloads hold
//! fewer than the reserve, a freed slot goes to the largest pending
//! download even while metadata is queued; beyond the reserve, queued
//! metadata wins. Both directions are work-conserving — either class
//! may use the whole pool while the other has nothing pending.

use std::{
    cmp::Ordering,
    collections::{BinaryHeap, VecDeque},
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;

use crate::UNPRIORITIZED;

/// Counting semaphore with the two-class grant policy described in the
/// module docs. Cancel-safe: dropping a waiting [`acquire`] future
/// gives up its place in line, and a permit granted to a waiter that
/// was cancelled in the same instant passes on to the next waiter
/// instead of leaking.
///
/// [`acquire`]: Self::acquire
pub(crate) struct PrioritySemaphore {
    state: Arc<Mutex<SemState>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Latency,
    Throughput,
}

impl Class {
    fn of(priority: u64) -> Class {
        if priority == UNPRIORITIZED { Class::Latency } else { Class::Throughput }
    }
}

struct SemState {
    free: usize,
    latency_in_flight: usize,
    throughput_in_flight: usize,
    /// Minimum number of slots queued throughput work can always
    /// grow into, even while latency work is queued.
    throughput_reserve: usize,
    /// Registration counter used as the FIFO tie-break.
    next_seq: u64,
    latency_waiters: VecDeque<Waiter>,
    throughput_waiters: BinaryHeap<Waiter>,
}

struct Waiter {
    priority: u64,
    seq: u64,
    tx: oneshot::Sender<Permit>,
}

/// `BinaryHeap` is a max-heap, so "greatest" pops first: order by
/// priority, then by *earlier* registration among equals.
impl Ord for Waiter {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority).then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for Waiter {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Waiter {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Waiter {}

/// Owned permit returned by [`PrioritySemaphore::acquire`]. Dropping it
/// hands the slot to the next waiter per the class policy (or back to
/// the free-permit count when nobody is waiting).
pub(crate) struct Permit {
    state: Arc<Mutex<SemState>>,
    class: Class,
    /// Cleared when the permit's release has been handed off inside
    /// [`release`] (a waiter vanished between pop and send), so this
    /// permit's own `Drop` must not release a second slot.
    armed: bool,
}

impl Drop for Permit {
    fn drop(&mut self) {
        if self.armed {
            release(&self.state, self.class);
        }
    }
}

impl PrioritySemaphore {
    pub(crate) fn new(permits: usize) -> Self {
        PrioritySemaphore {
            state: Arc::new(Mutex::new(SemState {
                free: permits,
                latency_in_flight: 0,
                throughput_in_flight: 0,
                // Half the pool, but never all of it: a reserve that
                // covers every permit (the `permits == 1` case) would
                // invert the starvation guarantee — queued downloads
                // would block metadata outright instead of sharing.
                throughput_reserve: permits.div_ceil(2).min(permits.saturating_sub(1)),
                next_seq: 0,
                latency_waiters: VecDeque::new(),
                throughput_waiters: BinaryHeap::new(),
            })),
        }
    }

    /// Wait for a permit. Free permits are claimed immediately; when
    /// the pool is saturated the caller queues in its class and is
    /// woken per the grant policy in the module docs.
    pub(crate) async fn acquire(&self, priority: u64) -> Permit {
        let class = Class::of(priority);
        let rx = {
            let mut state = self.state.lock().expect("priority semaphore lock poisoned");
            if state.free > 0 {
                state.free -= 1;
                *state.count_mut(class) += 1;
                return Permit { state: Arc::clone(&self.state), class, armed: true };
            }
            let (tx, rx) = oneshot::channel();
            let seq = state.next_seq;
            state.next_seq += 1;
            let waiter = Waiter { priority, seq, tx };
            match class {
                Class::Latency => state.latency_waiters.push_back(waiter),
                Class::Throughput => state.throughput_waiters.push(waiter),
            }
            rx
        };
        rx.await.expect("priority semaphore state dropped while a permit was awaited")
    }

    #[cfg(test)]
    pub(crate) fn queued_waiters(&self) -> usize {
        let state = self.state.lock().expect("priority semaphore lock poisoned");
        state.latency_waiters.len() + state.throughput_waiters.len()
    }

    #[cfg(test)]
    pub(crate) fn available_permits(&self) -> usize {
        self.state.lock().expect("priority semaphore lock poisoned").free
    }
}

impl SemState {
    fn count_mut(&mut self, class: Class) -> &mut usize {
        match class {
            Class::Latency => &mut self.latency_in_flight,
            Class::Throughput => &mut self.throughput_in_flight,
        }
    }

    /// Pop the waiter the grant policy picks next, or `None` when both
    /// queues are empty: throughput work below its reserve goes first,
    /// then queued latency work, then throughput work above the
    /// reserve.
    fn next_waiter(&mut self) -> Option<(Waiter, Class)> {
        if self.throughput_in_flight < self.throughput_reserve
            && let Some(waiter) = self.throughput_waiters.pop()
        {
            return Some((waiter, Class::Throughput));
        }
        if let Some(waiter) = self.latency_waiters.pop_front() {
            return Some((waiter, Class::Latency));
        }
        self.throughput_waiters.pop().map(|waiter| (waiter, Class::Throughput))
    }
}

impl std::fmt::Debug for PrioritySemaphore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().expect("priority semaphore lock poisoned");
        f.debug_struct("PrioritySemaphore")
            .field("free", &state.free)
            .field("latency_in_flight", &state.latency_in_flight)
            .field("throughput_in_flight", &state.throughput_in_flight)
            .field("latency_waiters", &state.latency_waiters.len())
            .field("throughput_waiters", &state.throughput_waiters.len())
            .finish()
    }
}

/// Hand a freed slot to the next waiter per the grant policy, skipping
/// waiters whose `acquire` future was dropped while queued; with no
/// live waiter the slot returns to the free-permit count.
fn release(state_arc: &Arc<Mutex<SemState>>, released: Class) {
    let mut state = state_arc.lock().expect("priority semaphore lock poisoned");
    *state.count_mut(released) -= 1;
    loop {
        let Some((waiter, class)) = state.next_waiter() else {
            state.free += 1;
            return;
        };
        let permit = Permit { state: Arc::clone(state_arc), class, armed: true };
        match waiter.tx.send(permit) {
            Ok(()) => {
                *state.count_mut(class) += 1;
                return;
            }
            Err(mut returned) => {
                // The receiver was dropped before the send, so this
                // permit was never observed — disarm it and offer the
                // slot to the next waiter. (Dropping it armed would
                // re-enter `release` and deadlock on `state_arc`.)
                returned.armed = false;
            }
        }
    }
}

#[cfg(test)]
mod tests;
