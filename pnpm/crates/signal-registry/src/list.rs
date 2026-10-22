//! The list spine both registries are built on.

use std::sync::atomic::{AtomicPtr, Ordering};

/// An append-only list of entries a signal handler can walk.
///
/// Each entry carries its own state, so what a registration means and when
/// an entry counts as free are the caller's to define: see
/// [`SignalRegistry`](crate::SignalRegistry) for the case where the entry
/// is a slot holding one value.
///
/// Entries are leaked, so an entry handed out stays valid for the life of
/// the process and a walk can hold one across anything.
pub struct SignalList<Entry: 'static> {
    head: AtomicPtr<Node<Entry>>,
}

struct Node<Entry: 'static> {
    entry: Entry,
    next: AtomicPtr<Node<Entry>>,
}

impl<Entry: 'static> SignalList<Entry> {
    /// An empty list, for a `static`.
    #[must_use]
    pub const fn new() -> Self {
        Self { head: AtomicPtr::new(std::ptr::null_mut()) }
    }

    /// Add `entry` to the list and hand back a reference to it that lives
    /// as long as the process.
    ///
    /// Allocates, so not for a signal handler. The entry's own state is
    /// published before the list head moves, so a concurrent walk either
    /// misses the entry or sees it whole.
    pub fn push(&self, entry: Entry) -> &'static Entry {
        // `into_raw` rather than `leak`: the raw pointer is what the list
        // stores and what every walk reads, so it has to be the one the
        // allocation is reached through. Deriving it from the `&mut` that
        // `leak` hands back instead would make every later write through a
        // shared reference to the node invalidate it, which Miri rejects
        // under Stacked Borrows.
        let node =
            Box::into_raw(Box::new(Node { entry, next: AtomicPtr::new(std::ptr::null_mut()) }));
        // SAFETY: `node` points at the allocation just created, which is
        // never freed, so the reference is good for the life of the
        // process. It is shared, and everything written through it is an
        // atomic, so it coexists with the raw pointer the list stores.
        let node = unsafe { &*node };
        let mut head = self.head.load(Ordering::Acquire);
        loop {
            node.next.store(head, Ordering::Release);
            match self.head.compare_exchange_weak(
                head,
                std::ptr::from_ref(node).cast_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return &node.entry,
                Err(current) => head = current,
            }
        }
    }

    /// Call `visit` with every entry ever added, newest first.
    ///
    /// Allocation-free and lock-free, so a signal handler can run it. An
    /// entry added during the walk may or may not be reached.
    pub fn for_each(&self, mut visit: impl FnMut(&'static Entry)) {
        let mut next = self.head.load(Ordering::Acquire);
        // SAFETY: every pointer in the list came from the `Box::leak` in
        // `push` and is never freed, so each stays dereferenceable for the
        // life of the process. The reference handed to `visit` is `'static`
        // for the same reason.
        while let Some(node) = unsafe { next.as_ref() } {
            visit(&node.entry);
            next = node.next.load(Ordering::Acquire);
        }
    }

    /// The first entry `accepts` returns `true` for, stopping there.
    ///
    /// Allocation-free and lock-free, as [`Self::for_each`] is.
    pub fn find(&self, mut accepts: impl FnMut(&'static Entry) -> bool) -> Option<&'static Entry> {
        let mut next = self.head.load(Ordering::Acquire);
        // SAFETY: as in `for_each`, every pointer in the list came from the
        // `Box::leak` in `push` and is never freed.
        while let Some(node) = unsafe { next.as_ref() } {
            if accepts(&node.entry) {
                return Some(&node.entry);
            }
            next = node.next.load(Ordering::Acquire);
        }
        None
    }

    /// Call `claim` with each entry in turn until one accepts, and add a
    /// new entry built by `build` if none does.
    ///
    /// This is how a slot is reused: `claim` is the caller's test of
    /// whether an entry is free, and taking it is theirs to make atomic.
    pub fn claim_or_push(
        &self,
        claim: impl FnMut(&'static Entry) -> bool,
        build: impl FnOnce() -> Entry,
    ) -> &'static Entry {
        self.find(claim)
            .unwrap_or_else(|| self.push(build()))
    }
}

impl<Entry: 'static> Default for SignalList<Entry> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
