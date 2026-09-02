//! Best-effort process-tree cleanup for failed and interrupted commands.
//!
//! Windows has no POSIX signals or process groups, so terminating a child
//! reaches only the direct child. A Job Object with `KILL_ON_JOB_CLOSE` lets
//! the OS terminate the whole process tree after an error, panic, or
//! interrupt. Successful commands disarm the job so a process deliberately
//! detached by a script can outlive pacquet. On Unix the kernel's
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

use std::process::Child;

#[cfg(windows)]
use std::sync::Mutex;

/// Process-tree cleanup armed for the lifetime of a command.
///
/// Call [`Self::disarm`] after a successful command to let detached descendants
/// survive. Otherwise the operating system closes the armed Job Object at
/// process exit and terminates its remaining processes. The guard is inert on
/// Unix.
pub struct JobGuard {
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
}

/// The armed job that spawned children join explicitly because pacquet itself
/// stayed out of it. `None` while pacquet is a member of its own job, so the
/// children inherit the membership, and after the job is disarmed.
#[cfg(windows)]
static CHILD_JOB: Mutex<Option<JobHandle>> = Mutex::new(None);

#[cfg(windows)]
struct JobHandle(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: a Job Object handle is a process-wide kernel handle with no
// thread affinity, so it can be used from any thread.
#[cfg(windows)]
unsafe impl Send for JobHandle {}

impl JobGuard {
    /// Disable process-tree cleanup after a successful command.
    #[cfg(windows)]
    pub fn disarm(self) {
        use core::mem::{size_of, zeroed};
        use core::ptr;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        // Stop assigning children first so no spawn can reach the handle
        // once it is closed.
        *CHILD_JOB.lock().expect("child job lock is not poisoned") = None;

        // SAFETY: `self.job` is the valid handle created by
        // [`arm_process_tree_cleanup`]. The information pointer refers to a
        // stack local that outlives the call. Clear the kill limit before
        // closing the handle; if clearing fails, leave the armed handle for
        // the operating system to close at exit.
        unsafe {
            let info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            let set = SetInformationJobObject(
                self.job,
                JobObjectExtendedLimitInformation,
                ptr::addr_of!(info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set != 0 {
                CloseHandle(self.job);
            }
        }
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
    use core::mem::{size_of, zeroed};
    use core::ptr;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: these are standard Win32 Job Object calls. Every pointer
    // argument is either null or a stack local that outlives the call,
    // `GetCurrentProcess` returns a pseudo-handle that must not be closed,
    // and the job handle is released by the OS at process exit.
    unsafe {
        let job = CreateJobObjectW(ptr::null(), ptr::null());
        if job.is_null() {
            return None;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            ptr::addr_of!(info).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if set == 0 {
            CloseHandle(job);
            return None;
        }
        if enclosing_job_releases_children() {
            *CHILD_JOB.lock().expect("child job lock is not poisoned") = Some(JobHandle(job));
        } else if AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
            // Fails when pacquet is already inside a job that forbids
            // nesting; fall back to no tree cleanup rather than aborting the
            // command.
            CloseHandle(job);
            return None;
        }
        // Deliberately do not close `job`: it must stay open until the process
        // exits so `KILL_ON_JOB_CLOSE` fires then. Closing it now would also
        // terminate this process when it is assigned to the job.
        Some(JobGuard { job })
    }
}

#[cfg(not(windows))]
#[must_use]
pub fn arm_process_tree_cleanup() -> Option<JobGuard> {
    Some(JobGuard {})
}

/// Whether pacquet is already a member of a job that lets its members'
/// children silently break away, so that nesting our own job would pin them
/// in it instead.
#[cfg(windows)]
fn enclosing_job_releases_children() -> bool {
    use core::mem::{size_of, zeroed};
    use core::ptr;
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

    let child_job = CHILD_JOB.lock().expect("child job lock is not poisoned");
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
