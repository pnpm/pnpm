//! Best-effort process-tree cleanup for failed and interrupted commands.
//!
//! Windows has no POSIX signals or process groups, so terminating a child
//! reaches only the direct child. A Job Object with `KILL_ON_JOB_CLOSE` lets
//! the OS terminate the whole process tree after an error, panic, or
//! interrupt. Successful commands disarm the job so a process deliberately
//! detached by a script can outlive pacquet. A descendant created with
//! `CREATE_BREAKAWAY_FROM_JOB` leaves the job, so it outlives pacquet even
//! after a failure. On Unix the kernel's
//! process-group and signal model already provides cleanup, so setup is a
//! no-op.
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
//! [`arm_process_tree_cleanup`] returns a guard to bind for the lifetime of
//! the process.

use crate::process::Child;

#[cfg(windows)]
use pnpm_ffi_process::Job;
#[cfg(windows)]
use std::sync::Mutex;

/// Process-tree cleanup armed for the lifetime of a command.
///
/// Call [`Self::disarm`] after a successful command to let detached
/// descendants survive. Otherwise the operating system closes the armed Job
/// Object at process exit and terminates its remaining processes. The guard
/// is inert on Unix.
pub struct JobGuard {
    #[cfg(windows)]
    job: &'static Job,
}

/// The armed job that spawned children join explicitly because pacquet
/// itself stayed out of it. `None` while pacquet is a member of its own
/// job, so the children inherit the membership, and after the job is
/// disarmed.
#[cfg(windows)]
static CHILD_JOB: Mutex<Option<&'static Job>> = Mutex::new(None);

impl JobGuard {
    /// Disable process-tree cleanup after a successful command.
    #[cfg(windows)]
    pub fn disarm(self) {
        // Stop assigning children first, so no spawn can reach the job once
        // it is no longer the one doing the cleanup.
        *CHILD_JOB.lock().expect("child job lock is not poisoned") = None;
        // A disarm that fails leaves the job armed, and the detached
        // processes in it are terminated at exit after all. There is nothing
        // better to do: the alternative is to keep waiting on them.
        let _ = self.job.disarm();
    }

    /// Disable the inert process-tree cleanup guard on Unix.
    #[cfg(not(windows))]
    pub fn disarm(self) {
        let Self {} = self;
    }
}

#[cfg(windows)]
#[must_use]
pub fn arm_process_tree_cleanup() -> Option<JobGuard> {
    // The job is leaked deliberately. An armed one has to stay open until
    // the process exits, so that the kill-on-close fires then rather than
    // when the guard goes; and a child assigned from another thread reads
    // the job through `CHILD_JOB`, which outlives the guard too.
    let job: &'static Job = Box::leak(Box::new(Job::with_tree_cleanup()?));
    if pnpm_ffi_process::enclosing_job_releases_children() {
        *CHILD_JOB.lock().expect("child job lock is not poisoned") = Some(job);
    } else if !job.assign_current_process() {
        // Fails when pacquet is already inside a job that forbids nesting;
        // fall back to no tree cleanup rather than aborting the command.
        return None;
    }
    Some(JobGuard { job })
}

#[cfg(not(windows))]
#[must_use]
pub fn arm_process_tree_cleanup() -> Option<JobGuard> {
    Some(JobGuard {})
}

/// Make a freshly spawned child a member of the armed job when pacquet
/// itself stayed out of it. Call it right after the spawn: a grandchild
/// created before the assignment is not covered.
#[cfg(windows)]
pub(crate) fn assign_child(child: &Child) {
    use std::os::windows::io::AsHandle as _;

    let child_job = CHILD_JOB.lock().expect("child job lock is not poisoned");
    let Some(job) = *child_job else { return };
    // A failed assignment only leaves that child outside the tree cleanup.
    let _ = job.assign(child.as_handle());
}

#[cfg(not(windows))]
pub(crate) fn assign_child(_: &Child) {}
