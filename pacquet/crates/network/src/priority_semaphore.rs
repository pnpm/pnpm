//! Priority-ordered permit dispenser backing
//! [`ThrottledClient`](crate::ThrottledClient).
//!
//! `tokio::sync::Semaphore` wakes waiters strictly FIFO, so when the
//! connection pool saturates, a multi-megabyte tarball queued behind a
//! burst of kilobyte-sized ones starts last — and a large archive that
//! starts last runs alone at single-connection throughput while every
//! other slot sits idle (the classic longest-processing-time-first
//! scheduling argument, see
//! [pnpm/pnpm#12230](https://github.com/pnpm/pnpm/issues/12230) for the
//! per-connection bandwidth measurements). This semaphore instead wakes
//! the highest-priority waiter first; tarball downloads pass their
//! `unpackedSize` as the priority so the biggest pending archives claim
//! freed slots before small ones. Waiters with equal priority are woken
//! FIFO, so callers that don't opt into prioritization keep the plain
//! semaphore's ordering among themselves.

use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;

/// Counting semaphore that grants queued permits highest-priority-first
/// instead of FIFO. Cancel-safe: dropping a waiting [`acquire`] future
/// gives up its place in line, and a permit granted to a waiter that
/// was cancelled in the same instant passes on to the next waiter
/// instead of leaking.
///
/// [`acquire`]: Self::acquire
pub(crate) struct PrioritySemaphore {
    state: Arc<Mutex<SemState>>,
}

struct SemState {
    permits: usize,
    /// Registration counter used as the FIFO tie-break between waiters
    /// of equal priority.
    next_seq: u64,
    waiters: BinaryHeap<Waiter>,
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
/// releases the slot to the highest-priority waiter (or back to the
/// free-permit count when nobody is waiting).
pub(crate) struct Permit {
    state: Arc<Mutex<SemState>>,
    /// Cleared when the permit's release has been handed off inside
    /// [`release`] (a waiter vanished between pop and send), so this
    /// permit's own `Drop` must not release a second slot.
    armed: bool,
}

impl Drop for Permit {
    fn drop(&mut self) {
        if self.armed {
            release(&self.state);
        }
    }
}

impl PrioritySemaphore {
    pub(crate) fn new(permits: usize) -> Self {
        PrioritySemaphore {
            state: Arc::new(Mutex::new(SemState {
                permits,
                next_seq: 0,
                waiters: BinaryHeap::new(),
            })),
        }
    }

    /// Wait for a permit. Free permits are claimed immediately; when
    /// the semaphore is saturated the caller queues and is woken
    /// highest-`priority`-first (FIFO among equal priorities).
    pub(crate) async fn acquire(&self, priority: u64) -> Permit {
        let rx = {
            let mut state = self.state.lock().expect("priority semaphore lock poisoned");
            if state.permits > 0 {
                state.permits -= 1;
                return Permit { state: Arc::clone(&self.state), armed: true };
            }
            let (tx, rx) = oneshot::channel();
            let seq = state.next_seq;
            state.next_seq += 1;
            state.waiters.push(Waiter { priority, seq, tx });
            rx
        };
        rx.await.expect("priority semaphore state dropped while a permit was awaited")
    }

    #[cfg(test)]
    pub(crate) fn queued_waiters(&self) -> usize {
        self.state.lock().expect("priority semaphore lock poisoned").waiters.len()
    }

    #[cfg(test)]
    pub(crate) fn available_permits(&self) -> usize {
        self.state.lock().expect("priority semaphore lock poisoned").permits
    }
}

impl std::fmt::Debug for PrioritySemaphore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().expect("priority semaphore lock poisoned");
        f.debug_struct("PrioritySemaphore")
            .field("permits", &state.permits)
            .field("waiters", &state.waiters.len())
            .finish()
    }
}

/// Hand a freed slot to the highest-priority live waiter, skipping
/// waiters whose `acquire` future was dropped while queued; with no
/// live waiter the slot returns to the free-permit count.
fn release(state_arc: &Arc<Mutex<SemState>>) {
    let mut state = state_arc.lock().expect("priority semaphore lock poisoned");
    loop {
        let Some(waiter) = state.waiters.pop() else {
            state.permits += 1;
            return;
        };
        let permit = Permit { state: Arc::clone(state_arc), armed: true };
        match waiter.tx.send(permit) {
            Ok(()) => return,
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
