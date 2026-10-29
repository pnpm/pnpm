//! Process-tree cleanup for when pnpm is killed, and containment for the
//! tasks a bail cancels. The contract these implement is in
//! `PROCESS_LIFETIME.md` at the root of this crate.
//!
//! Windows has no POSIX signals or process groups, so terminating a child
//! reaches only the direct child. A Job Object with `KILL_ON_JOB_CLOSE` lets
//! the OS terminate the whole process tree when pnpm is killed or crashes.
//! Whenever pnpm exits on its own, [`crate::exit`] or
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
//! A task that a bail may cancel also gets a `TaskJob` of its own, nested
//! in pnpm's job, so the cancellation ends everything the task started
//! without walking parent links, which a recycled pid could misdirect.

use crate::process::Child;

#[cfg(windows)]
use std::sync::{Arc, Mutex, PoisonError};

/// The armed job, until the cleanup is released.
#[cfg(windows)]
static ARMED_JOB: Mutex<Option<JobHandle>> = Mutex::new(None);

/// The armed job that spawned children join explicitly because pacquet itself
/// stayed out of it. `None` while pacquet is a member of its own job, so the
/// children inherit the membership, and after the cleanup is released.
#[cfg(windows)]
static CHILD_JOB: Mutex<Option<JobHandle>> = Mutex::new(None);

#[cfg(windows)]
struct JobHandle(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: a Job Object handle is a process-wide kernel handle with no
// thread affinity, so it can be used from any thread.
#[cfg(windows)]
unsafe impl Send for JobHandle {}

// SAFETY: as for `Send`; every call made through a shared handle is a
// thread-safe kernel call.
#[cfg(windows)]
unsafe impl Sync for JobHandle {}

/// Tie every process pnpm starts from now on to pnpm, so they end with it if
/// it is killed or crashes. [`crate::exit`] releases them again on every exit pnpm
/// makes on its own. A failure to arm leaves the processes untied rather than
/// failing the command.
#[cfg(windows)]
pub fn arm_process_tree_cleanup() {
    use std::{
        mem::{size_of, zeroed},
        ptr,
    };
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut armed = ARMED_JOB.lock().unwrap_or_else(PoisonError::into_inner);
    if armed.is_some() {
        return;
    }
    // SAFETY: these are standard Win32 Job Object calls. Every pointer
    // argument is either null or a stack local that outlives the call,
    // `GetCurrentProcess` returns a pseudo-handle that must not be closed,
    // and the job handle is released by the OS at process exit.
    unsafe {
        let job = CreateJobObjectW(ptr::null(), ptr::null());
        if job.is_null() {
            return;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        // Not `SILENT_BREAKAWAY_OK`: that would release every descendant and
        // leave nothing for `KILL_ON_JOB_CLOSE` to clean up.
        info.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
        let set = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            ptr::addr_of!(info).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if set == 0 {
            CloseHandle(job);
            return;
        }
        if enclosing_job_releases_children() {
            *CHILD_JOB.lock().unwrap_or_else(PoisonError::into_inner) = Some(JobHandle(job));
        } else if AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
            // Fails when pacquet is already inside a job that forbids
            // nesting; fall back to no tree cleanup rather than aborting the
            // command.
            CloseHandle(job);
            return;
        }
        // Deliberately do not close `job`: it must stay open until the process
        // exits so `KILL_ON_JOB_CLOSE` fires then. Closing it now would also
        // terminate this process when it is assigned to the job.
        *armed = Some(JobHandle(job));
    }
}

#[cfg(not(windows))]
pub fn arm_process_tree_cleanup() {}

/// Untie the processes pnpm started from pnpm's own lifetime, so they keep
/// running after it exits. [`crate::exit`] calls this, and so does every other
/// path on which pnpm decides to end: the end of `main`, and a closing
/// console, whose own processes the operating system ends anyway.
#[cfg(windows)]
pub(crate) fn release_process_tree_cleanup() {
    use std::{
        mem::{size_of, zeroed},
        ptr,
    };
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    // Stop assigning children first so no spawn can reach the handle once
    // it is closed.
    *CHILD_JOB.lock().unwrap_or_else(PoisonError::into_inner) = None;
    let Some(JobHandle(job)) = ARMED_JOB
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    else {
        return;
    };
    // SAFETY: `job` is the valid handle created by
    // [`arm_process_tree_cleanup`]. The information pointer refers to a
    // stack local that outlives the call. Clear the kill limit before
    // closing the handle; if clearing fails, leave the armed handle for
    // the operating system to close at exit. Breakaway stays allowed for
    // the processes that remain in the job.
    unsafe {
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_BREAKAWAY_OK;
        let set = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            ptr::addr_of!(info).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if set != 0 {
            CloseHandle(job);
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn release_process_tree_cleanup() {}

/// Whether pacquet is already a member of a job that lets its members'
/// children silently break away, so that nesting our own job would pin them
/// in it instead.
#[cfg(windows)]
fn enclosing_job_releases_children() -> bool {
    use std::{
        mem::{size_of, zeroed},
        ptr,
    };
    use windows_sys::Win32::System::JobObjects::{
        IsProcessInJob, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, QueryInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: every pointer argument is null or a stack local that outlives
    // the call. A null job handle addresses the immediate job of the calling
    // process.
    unsafe {
        let mut in_job = 0;
        if IsProcessInJob(GetCurrentProcess(), ptr::null_mut(), &raw mut in_job) == 0 || in_job == 0
        {
            return false;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        let queried = QueryInformationJobObject(
            ptr::null_mut(),
            JobObjectExtendedLimitInformation,
            ptr::addr_of_mut!(info).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ptr::null_mut(),
        );
        queried != 0
            && info.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK != 0
    }
}

/// Make a freshly spawned child a member of the armed job when pacquet itself
/// stayed out of it. Call it right after the spawn: a grandchild created
/// before the assignment is not covered.
#[cfg(windows)]
pub(crate) fn assign_child(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

    let child_job = CHILD_JOB.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(job) = child_job.as_ref() else { return };
    // SAFETY: `job.0` is the open handle created by
    // [`arm_process_tree_cleanup`], kept alive by the lock held across the
    // call, and the child handle is owned by `child`. A failed assignment
    // only leaves that child outside the tree cleanup.
    unsafe {
        AssignProcessToJobObject(job.0, child.as_raw_handle());
    }
}

#[cfg(not(windows))]
pub(crate) fn assign_child(_: &Child) {}

/// A Job Object holding one task's process tree, nested in pnpm's own job.
/// [`Self::terminate`] ends everything the task started, wherever its parent
/// links lead. Closing the job ends nothing: a task that finishes on its own
/// leaves its leftovers running, as any command does.
#[cfg(windows)]
#[derive(Clone)]
pub(crate) struct TaskJob(Arc<OwnedJob>);

#[cfg(windows)]
struct OwnedJob(JobHandle);

#[cfg(windows)]
impl Drop for OwnedJob {
    fn drop(&mut self) {
        // SAFETY: the handle was created by `TaskJob::contain` and is closed
        // once, by the last owner.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle((self.0).0);
        }
    }
}

#[cfg(windows)]
impl TaskJob {
    /// Put a freshly spawned `child` in a job of its own. Call it right after
    /// the spawn, and after [`assign_child`]: a grandchild created before the
    /// assignment is not covered. `None` when the job cannot be made, which
    /// leaves the task to [`crate::ProcessTracker`]'s fallback.
    pub(crate) fn contain(child: &Child) -> Option<Self> {
        use std::{
            mem::{size_of, zeroed},
            os::windows::io::AsRawHandle,
            ptr,
        };
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        // SAFETY: standard Win32 Job Object calls. Every pointer argument is
        // null or a stack local that outlives the call, and the child handle
        // is owned by `child`. The job handle is owned by the returned value,
        // which closes it on every path below.
        unsafe {
            let job = CreateJobObjectW(ptr::null(), ptr::null());
            if job.is_null() {
                return None;
            }
            let job = OwnedJob(JobHandle(job));
            // Breakaway must be allowed by every job of the hierarchy, or a
            // descendant asking for it fails to start (pnpm/pnpm#16628).
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_BREAKAWAY_OK;
            let set = SetInformationJobObject(
                (job.0).0,
                JobObjectExtendedLimitInformation,
                ptr::addr_of!(info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            (set != 0 && AssignProcessToJobObject((job.0).0, child.as_raw_handle()) != 0).then(
                || Self(Arc::new(job)),
            )
        }
    }

    /// End every process of the task.
    pub(crate) fn terminate(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        // SAFETY: the handle stays open for as long as `self` holds it. On a
        // job whose processes have all exited the call is a no-op.
        unsafe {
            TerminateJobObject((self.0.0).0, 1);
        }
    }
}
