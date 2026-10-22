//! Changing the process environment.
//!
//! `setenv` and its Windows counterpart mutate a table the whole process
//! shares, and the standard library hands out no lock for it: a change
//! racing a read from another thread is undefined, and the read can come
//! from inside a library the caller never calls directly. So the change is
//! sound only while no other thread exists, and [`SingleThreaded`] is how
//! a caller shows that.

/// Evidence that no thread but the main one exists yet.
///
/// Claim it at the top of `main`, before a runtime, a thread pool, or
/// anything else that spawns, and spend it on the change the process
/// needs.
///
/// What the type carries: it is neither `Send` nor `Sync`, so it cannot
/// reach a spawned thread, and [`Self::set_var`] takes it by value, so it
/// is spent at the first call and cannot be kept for a second one. What it
/// cannot carry is that the claiming thread has spawned nothing between
/// the claim and the call, because no value can observe that. Keeping the
/// two adjacent is the caller's part, and moving the witness into the call
/// makes every later use a compile error.
#[must_use = "the witness is the proof that the environment may be changed"]
pub struct SingleThreaded(std::marker::PhantomData<*const ()>);

impl SingleThreaded {
    /// Claim the witness, which only the first caller gets.
    ///
    /// A later call hands back `None`, so a caller that reaches for one
    /// mid-run cannot mint it and quietly race a reader.
    pub fn claim() -> Option<Self> {
        static CLAIMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        CLAIMED
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
            .then_some(Self(std::marker::PhantomData))
    }

    /// Set `name` to `value` in the process environment, where every later
    /// read of it will see the new value, this process's children included.
    ///
    /// Takes the witness by value, so a process makes one such change and
    /// cannot hold the proof for a second one after it has spawned.
    pub fn set_var(self, name: &str, value: &str) {
        // SAFETY: the race `setenv` has is a concurrent read from another
        // thread. The witness is neither `Send` nor `Sync`, so this runs on
        // the thread that claimed it, and the claim is the first in the
        // process; that the caller spends it before spawning is the
        // obligation `SingleThreaded` documents.
        unsafe { std::env::set_var(name, value) };
    }
}

#[cfg(test)]
mod tests;
