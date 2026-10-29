//! Process-tree cleanup for when pnpm is killed, and containment for the
//! tasks a bail cancels. The contract these implement is in
//! `PROCESS_LIFETIME.md` at the root of this crate.
//!
//! Windows has no POSIX signals or process groups, so terminating a child
//! reaches only the direct child. A Job Object with `KILL_ON_JOB_CLOSE` lets
//! the OS terminate the whole process tree when pnpm is killed or crashes.
//! Whenever pnpm exits on its own, [`crate::exit()`] or
//! [`release_process_tree_cleanup`] clears the kill limit first, so whatever
//! a command left running outlives pnpm, as it does when the command runs
//! without pnpm. Cargo and `turbo exec` follow the same rule. A descendant
//! created with `CREATE_BREAKAWAY_FROM_JOB` leaves the job and outlives even
//! a pnpm that is killed. On Unix the kernel's process-group and signal model
//! already provides cleanup, so arming is a no-op.
//!
//! pacquet normally joins the job itself, so every descendant inherits the
//! membership. A parent such as Node's libuv may already have placed pacquet
//! in a job whose members' children silently break away from it. Joining our
//! own job would nest it inside that one, and a nested job that forbids
//! breakaway pins every descendant in the parent's job until the parent
//! exits, whether or not the command succeeded. In that case pacquet stays
//! out of its own job and assigns the children it spawns instead: they take
//! the breakaway out of the parent's job and belong to pacquet's job alone.
//!
//! A task that a bail may cancel also gets a Job Object of its own, nested in
//! pnpm's, so the cancellation ends everything the task started without
//! walking parent links, which a recycled pid could misdirect.
//!
//! A child joins a job it is assigned to by hand right after it is created,
//! so a process the child starts before that is not covered. The child would
//! have to start one within moments of its own creation. Closing that window
//! needs the child to be created inside the job, through
//! `PROC_THREAD_ATTRIBUTE_JOB_LIST`, which stable Rust's `Command` cannot pass
//! until `CommandExt::raw_attribute` is stable. Creating the child suspended
//! instead would overwrite a caller's creation flags, and resuming it would
//! take a walk of every thread on the system for each child.

use crate::process::Child;

#[cfg(windows)]
use pnpm_ffi_process::Job;
#[cfg(windows)]
use std::sync::{Arc, Mutex, PoisonError};

/// The armed job, until the cleanup is released. Tests read it to see whether
/// a path released the cleanup.
#[cfg(windows)]
pub(crate) static ARMED_JOB: Mutex<Option<Arc<Job>>> = Mutex::new(None);

/// The armed job that spawned children join explicitly because pacquet itself
/// stayed out of it. `None` while pacquet is a member of its own job, so the
/// children inherit the membership, and after the cleanup is released.
#[cfg(windows)]
static CHILD_JOB: Mutex<Option<Arc<Job>>> = Mutex::new(None);

/// Tie every process pnpm starts from now on to pnpm, so they end with it if
/// it is killed or crashes. Every exit pnpm makes on its own releases them
/// again, through [`crate::exit()`] or the end of `main`. A failure to arm
/// leaves the processes untied rather than failing the command.
#[cfg(windows)]
pub fn arm_process_tree_cleanup() {
    let mut armed = ARMED_JOB.lock().unwrap_or_else(PoisonError::into_inner);
    if armed.is_some() {
        return;
    }
    let Some(job) = Job::with_tree_cleanup().map(Arc::new) else {
        return;
    };
    if pnpm_ffi_process::enclosing_job_releases_children() {
        *CHILD_JOB.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&job));
    } else if !job.assign_current_process() {
        // Fails when pacquet is already inside a job that forbids nesting;
        // fall back to no tree cleanup rather than aborting the command.
        // Closing the job here ends nothing, since the join that would have
        // put this process in it is what failed.
        return;
    }
    // The job is deliberately held rather than dropped: it must stay open
    // until the process exits so its kill-on-close fires then.
    *armed = Some(job);
}

#[cfg(not(windows))]
pub fn arm_process_tree_cleanup() {}

/// Untie the processes pnpm started from pnpm's own lifetime, so they keep
/// running after it exits. [`crate::exit()`] calls this, and so does every
/// other path on which pnpm decides to end: the end of `main`, a closing
/// console, whose own processes the operating system ends anyway, and an
/// interrupt while pnpm runs no command.
#[cfg(windows)]
pub(crate) fn release_process_tree_cleanup() {
    // Take the locks in the order arming takes them, and hold the armed job's
    // across both, so a release that races the arming waits for it rather
    // than leaving a job armed behind it. Children stop being assigned
    // first, so no spawn can reach the job once it is closed.
    let mut armed = ARMED_JOB.lock().unwrap_or_else(PoisonError::into_inner);
    *CHILD_JOB.lock().unwrap_or_else(PoisonError::into_inner) = None;
    let Some(job) = armed.take() else {
        return;
    };
    drop(armed);
    // Clear the kill limit before the handle closes. Breakaway stays allowed
    // for the processes that remain in the job. If clearing fails, hold the
    // job open for the operating system to close at exit instead, since
    // closing an armed one terminates what is still in it.
    if !job.disarm() {
        let _armed = std::mem::ManuallyDrop::new(job);
    }
}

#[cfg(not(windows))]
pub(crate) fn release_process_tree_cleanup() {}

/// Make a freshly spawned child a member of the armed job when pacquet itself
/// stayed out of it. Call it right after the spawn: a process the child
/// starts before the assignment is not covered (see the module docs).
#[cfg(windows)]
pub(crate) fn assign_child(child: &Child) {
    use std::os::windows::io::AsHandle as _;

    let child_job = CHILD_JOB.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(job) = child_job.as_ref() else { return };
    // A failed assignment only leaves that child outside the tree cleanup.
    let _ = job.assign(child.as_handle());
}

#[cfg(not(windows))]
pub(crate) fn assign_child(_: &Child) {}

/// A Job Object holding one task's process tree, nested in pnpm's own job.
/// [`Self::terminate`] ends everything the task started, wherever its parent
/// links lead. Closing the job ends nothing: a task that finishes on its own
/// leaves its leftovers running, as any command does.
#[cfg(windows)]
#[derive(Clone)]
pub(crate) struct TaskJob(Arc<Job>);

#[cfg(windows)]
impl TaskJob {
    /// Put a freshly spawned `child` in a job of its own, right after
    /// [`assign_child`]. `None` when the job cannot be made, which leaves the
    /// task to [`crate::ProcessTracker`]'s fallback.
    pub(crate) fn contain(child: &Child) -> Option<Self> {
        use std::os::windows::io::AsHandle as _;

        let job = Job::new()?;
        job.assign(child.as_handle())
            .then(|| Self(Arc::new(job)))
    }

    /// End every process of the task.
    pub(crate) fn terminate(&self) {
        self.0.terminate();
    }
}
