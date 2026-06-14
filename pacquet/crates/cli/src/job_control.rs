//! Best-effort process-tree cleanup, mirroring Cargo and Bun.
//!
//! Windows has no POSIX signals or process groups, so terminating a child
//! reaches only the direct child — a grandchild spawned by a lifecycle script
//! that outlives a failed or interrupted command would be orphaned. Assigning
//! the pacquet process to a Job Object with `KILL_ON_JOB_CLOSE` ties every
//! descendant's lifetime to ours: when pacquet exits — cleanly, on error, or
//! when killed — the OS terminates the whole tree. On Unix the kernel's
//! process-group and signal model already provides this, so setup is a no-op.
//!
//! [`setup`] returns a guard to bind for the lifetime of the process.

/// Marker that process-tree cleanup has been installed; hold it for the
/// lifetime of the process.
///
/// On Windows it stands for a Job Object whose handle is intentionally kept
/// open for the whole run: the OS closes it at exit, which is what fires
/// `KILL_ON_JOB_CLOSE`. On Unix the kernel's process-group and signal model
/// already tears down descendants, so the guard is inert.
pub struct JobGuard;

#[cfg(windows)]
pub fn setup() -> Option<JobGuard> {
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
        // Fails when pacquet is already inside a job that forbids nesting;
        // fall back to no tree cleanup rather than aborting the command.
        if AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
            CloseHandle(job);
            return None;
        }
        // Deliberately do not close `job`: it must stay open until the process
        // exits so `KILL_ON_JOB_CLOSE` fires then. Closing it now would also
        // terminate this process, since it is assigned to the job.
        Some(JobGuard)
    }
}

#[cfg(not(windows))]
pub fn setup() -> Option<JobGuard> {
    Some(JobGuard)
}
