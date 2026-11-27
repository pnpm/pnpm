//! Changing the process environment.
//!
//! `setenv` and its Windows counterpart mutate a table the whole process
//! shares, and the standard library hands out no lock for it: a change
//! racing a read from another thread is undefined, and the read can come
//! from inside a library the caller never calls directly. So the change is
//! sound only while no other thread can read the table.
//!
//! [`SingleThreaded::set_vars`] establishes that per platform:
//!
//! - Linux, Android, macOS, FreeBSD, NetBSD: the process has one thread
//!   when the writes start. Only that thread could spawn another, and it
//!   is busy writing.
//! - Windows: nothing to check. The standard library documents `set_var`
//!   as always safe there.
//! - WebAssembly, any other Unix, and a count the kernel does not report:
//!   the [`SingleThreaded`] witness alone.

#[cfg(unix)]
mod thread_count;

/// Evidence that the holder is the only thread able to reach the process
/// environment.
///
/// Claim it at the top of `main`, before a runtime, a thread pool, or
/// anything else that spawns, and pass it to each change the process
/// makes during startup.
///
/// [`Self::claim`] hands out one per process, and the type is neither
/// `Send` nor `Sync`, so the witness stays on the thread that claimed it.
/// Where the thread count can be read, [`Self::set_vars`] checks it before
/// writing, and the witness only marks the stretch of `main` the writes
/// belong to. Where it cannot, the witness is the whole argument: nothing
/// checks that the holder has spawned nothing since the claim, so keeping
/// the claim and the writes adjacent, ahead of the first spawn, is the
/// caller's part.
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

    /// Set `name` to `value`, as [`Self::set_vars`] does.
    ///
    /// # Errors
    ///
    /// See [`Self::set_vars`].
    pub fn set_var(&self, name: &str, value: &str) -> Result<(), OtherThreads> {
        self.set_vars(vec![(name.to_owned(), value.to_owned())])
    }

    /// Set each name to its value in the process environment, in order,
    /// where every later read sees the new values, this process's children
    /// included.
    ///
    /// The threads are counted once, before the first write.
    ///
    /// # Errors
    ///
    /// Refuses every write, leaving the environment unchanged, when another
    /// thread runs.
    pub fn set_vars(&self, vars: Vec<(String, String)>) -> Result<(), OtherThreads> {
        if vars.is_empty() {
            return Ok(());
        }
        ensure_sole_thread()?;
        for (name, value) in vars {
            #[expect(
                clippy::disallowed_methods,
                reason = "the one place the workspace writes the environment, which is what the \
                          witness this takes is for"
            )]
            // SAFETY: the race `setenv` has is a concurrent read from
            // another thread, and none can run here:
            // - Where the kernel reports a count, `ensure_sole_thread` just
            //   read a count of one. Only this thread could spawn
            //   another, and between the count and the last write it runs
            //   only these writes and std's `String` code, which spawns
            //   nothing.
            // - On Windows, std documents `set_var` as always safe.
            // - Elsewhere, and where the count is unknown, the witness was
            //   claimed at the top of `main`, ahead of every spawn, and it
            //   cannot leave that thread.
            unsafe {
                std::env::set_var(name, value);
            }
        }
        Ok(())
    }
}

/// Raised when [`SingleThreaded::set_vars`] refuses to change the
/// environment because other threads run, and one of them could read it
/// while it changes.
#[derive(Debug, derive_more::Display, derive_more::Error)]
#[display(
    "Cannot change the process environment: the process runs {threads} threads, and one of them \
     could read the environment while it changes"
)]
pub struct OtherThreads {
    pub threads: usize,
}

#[cfg(unix)]
fn ensure_sole_thread() -> Result<(), OtherThreads> {
    match thread_count::thread_count() {
        None | Some(1) => Ok(()),
        Some(threads) => Err(OtherThreads { threads }),
    }
}

#[cfg(not(unix))]
fn ensure_sole_thread() -> Result<(), OtherThreads> {
    Ok(())
}

#[cfg(test)]
mod tests;
