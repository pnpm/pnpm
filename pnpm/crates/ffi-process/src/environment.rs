//! Changing the process environment.
//!
//! `setenv` and its Windows counterpart mutate a table the whole process
//! shares, and the standard library hands out no lock for it: a change
//! racing a read from another thread is undefined, and the read can come
//! from inside a library the caller never calls directly. So the change is
//! sound only while no other thread can read the table.
//!
//! [`SingleThreaded::set_var`] establishes that per platform:
//!
//! - Linux, Android, macOS, FreeBSD, NetBSD: the process has one thread
//!   when the write starts. Only that thread could spawn another, and it
//!   is busy writing.
//! - Windows: nothing to check. The standard library documents `set_var`
//!   as always safe there.
//! - WebAssembly: the [`SingleThreaded`] witness. The guest cannot count
//!   its threads.
//! - Any other Unix: nothing, so the write is refused.

#[cfg(unix)]
mod thread_count;

use std::fmt;

/// Evidence that the holder is the only thread able to reach the process
/// environment.
///
/// Claim it at the top of `main`, before a runtime, a thread pool, or
/// anything else that spawns, and pass it to each change the process
/// makes during startup.
///
/// [`Self::claim`] hands out one per process, and the type is neither
/// `Send` nor `Sync`, so the witness stays on the thread that claimed it.
/// Where the thread count can be read, [`Self::set_var`] checks it before
/// every write, and the witness only marks the stretch of `main` the writes
/// belong to. On WebAssembly it is the whole argument: nothing checks that
/// the holder has spawned nothing since the claim, so keeping the claim
/// and the writes adjacent, ahead of the first spawn, is the caller's part.
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
    /// read of it sees the new value, this process's children included.
    ///
    /// # Errors
    ///
    /// Refuses the write, leaving the environment unchanged, when another
    /// thread runs or the thread count cannot be read.
    pub fn set_var(&self, name: &str, value: &str) -> Result<(), SetVarError> {
        ensure_sole_thread(name)?;
        #[expect(
            clippy::disallowed_methods,
            reason = "the one place the workspace writes the environment, which is what the \
                      witness this takes is for"
        )]
        // SAFETY: the race `setenv` has is a concurrent read from another
        // thread. `ensure_sole_thread` returned, so no other thread exists,
        // or the platform needs no check; the table in the module docs
        // gives the argument for each.
        unsafe {
            std::env::set_var(name, value);
        }
        Ok(())
    }
}

/// Why [`SingleThreaded::set_var`] refused to change a variable.
#[derive(Debug)]
#[non_exhaustive]
pub enum SetVarError {
    /// Other threads run, and one of them could read the environment while
    /// it changes.
    OtherThreads { name: String, threads: usize },
    /// The process's thread count could not be read.
    UncountedThreads { name: String, source: std::io::Error },
}

impl fmt::Display for SetVarError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OtherThreads { name, threads } => write!(
                formatter,
                "Cannot set the environment variable {name}: the process runs {threads} threads, \
                 and one of them could read the environment while it changes",
            ),
            Self::UncountedThreads { name, source } => write!(
                formatter,
                "Cannot set the environment variable {name}: failed to count the process's \
                 threads: {source}",
            ),
        }
    }
}

impl std::error::Error for SetVarError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OtherThreads { .. } => None,
            Self::UncountedThreads { source, .. } => Some(source),
        }
    }
}

#[cfg(unix)]
fn ensure_sole_thread(name: &str) -> Result<(), SetVarError> {
    match thread_count::thread_count() {
        Ok(1) => Ok(()),
        Ok(threads) => Err(SetVarError::OtherThreads { name: name.to_owned(), threads }),
        Err(source) => Err(SetVarError::UncountedThreads { name: name.to_owned(), source }),
    }
}

#[cfg(not(unix))]
fn ensure_sole_thread(_name: &str) -> Result<(), SetVarError> {
    Ok(())
}

#[cfg(test)]
mod tests;
