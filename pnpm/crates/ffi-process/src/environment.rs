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
/// anything else that spawns, and carry it to each change the process
/// makes during startup.
///
/// What the type carries: [`Self::claim`] hands out one per process, and
/// the type is `Send` but not `Sync`, so the witness travels the startup
/// path by move and one thread holds it at a time. What it cannot carry is
/// that the holder has spawned nothing since the claim, because no value
/// can observe that. Keeping the claim and the changes adjacent is the
/// caller's part.
#[must_use = "the witness is the proof that the environment may be changed"]
pub struct SingleThreaded(std::marker::PhantomData<std::cell::Cell<()>>);

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
    pub fn set_var(&self, name: &str, value: &str) {
        // SAFETY: the race `setenv` has is a concurrent read from another
        // thread. One witness exists and it is not `Sync`, so this runs on
        // the thread holding it; that nothing else is running yet is the
        // obligation `SingleThreaded` documents.
        unsafe { std::env::set_var(name, value) };
    }
}

#[cfg(test)]
mod tests;
