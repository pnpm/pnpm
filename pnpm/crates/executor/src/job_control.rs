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
//! An interrupt or a closed console ends only the job's processes that share
//! pacquet's console, which the console's event has reached as well. The rest
//! are released, so a browser a script opened outlives `Ctrl+C`, as it does
//! when the command runs without pnpm
//! ([pnpm/pnpm#17465](https://github.com/pnpm/pnpm/issues/17465)).
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
use std::sync::{Mutex, PoisonError};

/// Process-tree cleanup armed for the lifetime of a command.
///
/// Call [`Self::disarm`] after a successful command to let detached descendants
/// survive, or [`Self::release_after_interrupt`] after a command the user
/// interrupted. Otherwise the operating system closes the armed Job Object at
/// process exit and terminates its remaining processes. The guard is inert on
/// Unix.
pub struct JobGuard {}

/// The armed job, until it is disarmed or released after an interrupt. The
/// console's control handler reaches it here, as it holds no guard.
#[cfg(windows)]
static ARMED_JOB: Mutex<Option<JobHandle>> = Mutex::new(None);

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
        let Self {} = self;
        if let Some(job) = take_armed_job() {
            clear_kill_limit(&job);
        }
    }

    /// Disable the inert process-tree cleanup guard on Unix.
    #[cfg(not(windows))]
    pub fn disarm(self) {
        let Self {} = self;
    }

    /// End the job's processes that share pacquet's console after an
    /// interrupt, and release the rest. See [`release_after_interrupt`].
    pub fn release_after_interrupt(self) {
        let Self {} = self;
        release_after_interrupt();
    }
}

/// Take the armed job, and stop assigning children to it first so no spawn
/// can reach the handle once it is closed.
#[cfg(windows)]
fn take_armed_job() -> Option<JobHandle> {
    *CHILD_JOB.lock().unwrap_or_else(PoisonError::into_inner) = None;
    ARMED_JOB
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

/// Clear the kill limit of `job` and close it, so its processes run on.
#[cfg(windows)]
fn clear_kill_limit(job: &JobHandle) {
    use std::{
        mem::{size_of, zeroed},
        ptr,
    };
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    let job = job.0;
    // SAFETY: `job` is the valid handle created by
    // [`arm_process_tree_cleanup`]. The information pointer refers to a
    // stack local that outlives the call. Clear the kill limit before
    // closing the handle; if clearing fails, leave the armed handle for
    // the operating system to close at exit. Breakaway stays allowed for
    // the detached processes that remain in the job.
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

/// End the armed job's processes that share pacquet's console, then release
/// the rest instead of letting the job's close end them.
///
/// The console delivers an interrupt or a close to every process attached to
/// it, so those processes belong to the command and have been told to stop;
/// ending them is what the job's close would have done. A process outside
/// the console never saw the event: a GUI application such as the browser
/// `open` starts, or a program `start` gave a console of its own. Ending
/// the command does not end those when it runs without pnpm either.
///
/// Without a console to tell the two apart, the job stays armed and ends
/// everything at exit.
#[cfg(windows)]
pub(crate) fn release_after_interrupt() {
    let Some(console) = console_process_ids() else { return };
    let Some(job) = take_armed_job() else { return };
    end_console_members(&job, &console);
    clear_kill_limit(&job);
}

#[cfg(not(windows))]
pub(crate) fn release_after_interrupt() {}

/// The processes attached to pacquet's console, or `None` when it has none.
#[cfg(windows)]
fn console_process_ids() -> Option<Vec<u32>> {
    use windows_sys::Win32::System::Console::GetConsoleProcessList;

    let mut ids = vec![0_u32; 64];
    loop {
        // SAFETY: the buffer holds `ids.len()` entries for the call to fill.
        let count = unsafe { GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) } as usize;
        if count == 0 {
            return None;
        }
        if count <= ids.len() {
            ids.truncate(count);
            return Some(ids);
        }
        ids.resize(count, 0);
    }
}

/// End every process of `job` that `console` lists, other than pacquet.
#[cfg(windows)]
fn end_console_members(job: &JobHandle, console: &[u32]) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        TerminateProcess,
    };

    // SAFETY: plain FFI call.
    let own_pid = unsafe { GetCurrentProcessId() };
    for pid in job_process_ids(job) {
        if pid == own_pid || !console.contains(&pid) {
            continue;
        }
        // SAFETY: the process handle is checked before use and closed below.
        // The membership check runs on that handle, which pins the process,
        // so a pid recycled since the listing is never ended.
        unsafe {
            let process =
                OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                continue;
            }
            let mut in_job = 0;
            if IsProcessInJob(process, job.0, &raw mut in_job) != 0 && in_job != 0 {
                TerminateProcess(process, crate::interrupt::STATUS_CONTROL_C_EXIT);
            }
            CloseHandle(process);
        }
    }
}

/// The ids of the processes in `job`, or none when they cannot be read.
#[cfg(windows)]
fn job_process_ids(job: &JobHandle) -> Vec<u32> {
    use std::{mem::size_of, ptr};
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList, QueryInformationJobObject,
    };

    // The list's two counts are followed by one `usize` per process, so a
    // `usize` buffer keeps every field aligned.
    let header = size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() / size_of::<usize>() - 1;
    let mut capacity = 64;
    loop {
        let mut buffer = vec![0_usize; header + capacity];
        let list = buffer.as_mut_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
        // SAFETY: the buffer is aligned for the list, as long as the length
        // passed, and outlives the call. A buffer too small for every process
        // fails the call but still reports how many there are.
        let (assigned, listed) = unsafe {
            QueryInformationJobObject(
                job.0,
                JobObjectBasicProcessIdList,
                list.cast(),
                (buffer.len() * size_of::<usize>()) as u32,
                ptr::null_mut(),
            );
            ((*list).NumberOfAssignedProcesses as usize, (*list).NumberOfProcessIdsInList as usize)
        };
        if assigned > capacity {
            capacity = assigned + 16;
            continue;
        }
        return buffer[header..header + listed.min(capacity)]
            .iter()
            .map(|&id| id as u32)
            .collect();
    }
}

#[cfg(windows)]
#[must_use]
pub fn arm_process_tree_cleanup() -> Option<JobGuard> {
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
        *ARMED_JOB.lock().expect("armed job lock is not poisoned") = Some(JobHandle(job));
        Some(JobGuard {})
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
