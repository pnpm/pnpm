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
/// anything else that spawns, and pass it to whatever changes the
/// environment. The value carries the proof: a function that takes one
/// cannot be called from a thread that was spawned after it was claimed,
/// because there was no way to obtain one by then.
#[must_use = "the witness is the proof that the environment may be changed"]
pub struct SingleThreaded(());

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
            .then_some(Self(()))
    }

    /// Set `name` to `value` in the process environment, where every later
    /// read of it will see the new value, this process's children included.
    pub fn set_var(&self, name: &str, value: &str) {
        // SAFETY: the witness exists, so this is the thread that claimed it
        // and no other thread had been spawned at that point. A thread
        // spawned since cannot be running this, because obtaining a witness
        // to call it with is no longer possible.
        unsafe { std::env::set_var(name, value) };
    }
}

#[cfg(test)]
mod tests;
