//! A registry of values, where a registration is a guard.

use crate::SignalList;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// A registry of values a signal handler can walk.
///
/// A value is registered for as long as the returned [`Registration`]
/// lives. Registering allocates; walking does not, so a handler can read
/// the registry while another thread registers or releases.
pub struct SignalRegistry<Value: 'static> {
    slots: SignalList<Slot<Value>>,
    walks: AtomicUsize,
}

struct Slot<Value: 'static> {
    /// The registered value, or null while the slot is free. The pointer
    /// comes from `Box::into_raw`, so the slot owns what it points at.
    value: AtomicPtr<Value>,
}

impl<Value: 'static> SignalRegistry<Value> {
    /// An empty registry, for a `static`.
    #[must_use]
    pub const fn new() -> Self {
        Self { slots: SignalList::new(), walks: AtomicUsize::new(0) }
    }

    /// Register `value` until the returned guard is dropped.
    ///
    /// Allocates, so not for a signal handler.
    pub fn register(&'static self, value: Value) -> Registration<'static, Value> {
        let value = Box::into_raw(Box::new(value));
        let slot = self.slots.claim_or_push(
            |slot| {
                slot.value
                    .compare_exchange(
                        std::ptr::null_mut(),
                        value,
                        Ordering::SeqCst,
                        Ordering::Acquire,
                    )
                    .is_ok()
            },
            || Slot { value: AtomicPtr::new(value) },
        );
        Registration { registry: self, slot }
    }

    /// Call `visit` with every registered value.
    ///
    /// Allocation-free and lock-free, so a signal handler can run it. A
    /// value released during the walk may or may not be visited, and is not
    /// freed until the walk ends.
    pub fn for_each(&self, mut visit: impl FnMut(&Value)) {
        self.walks.fetch_add(1, Ordering::SeqCst);
        self.slots.for_each(|slot| {
            let value = slot.value.load(Ordering::SeqCst);
            // SAFETY: a non-null pointer here came from the
            // `Box::into_raw` in `register`, and the walk this counts
            // itself into keeps it from being freed: `release` frees only
            // after reading a zero walk count, and the `SeqCst` order makes
            // that read and the store that emptied the slot visible to each
            // other, so a `release` that frees cannot have left this load
            // returning its pointer.
            if let Some(value) = unsafe { value.as_ref() } {
                visit(value);
            }
        });
        self.walks.fetch_sub(1, Ordering::SeqCst);
    }

    /// Empty `slot` and free what it held, unless a walk may still be
    /// reading it, in which case the value is leaked instead.
    ///
    /// Leaking is the price of a walk that takes no lock. A slot is reused
    /// by the next registration either way, so the leak is bounded by how
    /// often a release happens to land inside a walk.
    fn release(&self, slot: &Slot<Value>) {
        let released = slot.value.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if self.walks.load(Ordering::SeqCst) != 0 || released.is_null() {
            return;
        }
        // SAFETY: `released` is non-null, so it came from the
        // `Box::into_raw` in `register`. Only this call can have taken it
        // out of the slot, because a guard is dropped once and owns the one
        // slot it claimed. No walk holds it: see `for_each`.
        drop(unsafe { Box::from_raw(released) });
    }
}

impl<Value: 'static> Default for SignalRegistry<Value> {
    fn default() -> Self {
        Self::new()
    }
}

/// A registered value's claim on its slot. Dropping it unregisters the
/// value.
#[must_use = "the value is registered only while the guard is held"]
pub struct Registration<'registry, Value: 'static> {
    registry: &'registry SignalRegistry<Value>,
    slot: &'static Slot<Value>,
}

impl<Value: 'static> Drop for Registration<'_, Value> {
    fn drop(&mut self) {
        self.registry.release(self.slot);
    }
}

#[cfg(test)]
mod tests;
