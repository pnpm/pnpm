//! Best-effort process-tree cleanup for failed and interrupted commands.
//!
//! Windows has no POSIX signals or process groups, so terminating a child
//! reaches only the direct child. Assigning pacquet to a Job Object with
//! `KILL_ON_JOB_CLOSE` lets the OS terminate the whole process tree after an
//! error, panic, or interrupt. Successful commands disarm the job so a process
//! deliberately detached by a script can outlive pacquet. On Unix the kernel's
//! process-group and signal model already provides cleanup, so setup is a no-op.
//!
//! [`setup`] returns a guard to bind for the lifetime of the process.

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

        // SAFETY: `self.job` is the valid handle created by [`setup`]. The
        // information pointer refers to a stack local that outlives the call.
        // Clear the kill limit before closing the handle; if clearing fails,
        // leave the armed handle for the operating system to close at exit.
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
        Some(JobGuard { job })
    }
}

#[cfg(not(windows))]
pub fn setup() -> Option<JobGuard> {
    Some(JobGuard {})
}
