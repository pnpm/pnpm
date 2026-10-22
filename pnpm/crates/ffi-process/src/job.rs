//! Windows Job Objects, which give a process tree the cleanup that Unix
//! gets from process groups and signals.

use std::os::windows::io::{AsRawHandle as _, BorrowedHandle, FromRawHandle as _, OwnedHandle};
use windows_sys::Win32::{
    Foundation::HANDLE,
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
            JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        },
        Threading::GetCurrentProcess,
    },
};

/// A Job Object that terminates the processes still in it when its last
/// handle closes.
///
/// `OwnedHandle` carries the job, so it closes exactly once and crosses
/// threads without anything further being claimed about it.
pub struct Job(OwnedHandle);

impl Job {
    /// Create a job whose members are terminated when the job closes.
    ///
    /// A descendant created with `CREATE_BREAKAWAY_FROM_JOB` leaves the
    /// job, so a process a script deliberately detached survives. `None`
    /// when the job cannot be created or configured, which leaves the
    /// caller without tree cleanup rather than failing its command.
    #[must_use]
    pub fn with_tree_cleanup() -> Option<Self> {
        let job = create()?;
        // Not `SILENT_BREAKAWAY_OK`: that would release every descendant
        // and leave nothing for the kill-on-close to clean up.
        set_limits(
            job.as_raw_handle(),
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK,
        )
        .then_some(Self(job))
    }

    /// Stop terminating the job's members, and report whether the change
    /// took.
    ///
    /// For a command that succeeded, so that a process a script detached
    /// outlives pnpm. Breakaway stays allowed for the detached processes
    /// still in the job. A caller that gets `false` leaves the job armed
    /// rather than closing it, since closing an armed job terminates its
    /// members.
    #[must_use]
    pub fn disarm(&self) -> bool {
        set_limits(self.0.as_raw_handle(), JOB_OBJECT_LIMIT_BREAKAWAY_OK)
    }

    /// Put the calling process in the job, so that every descendant it
    /// spawns inherits the membership.
    ///
    /// `false` when the process is already in a job that forbids nesting.
    #[must_use]
    pub fn assign_current_process(&self) -> bool {
        // SAFETY: the job handle is owned by `self` and open for the call,
        // and `GetCurrentProcess` returns the current-process pseudo
        // handle, which is always valid and must not be closed.
        unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), GetCurrentProcess()) != 0 }
    }

    /// Put one process in the job, for when the calling process stayed out
    /// of it.
    ///
    /// Call it right after the spawn: a grandchild created before the
    /// assignment is not covered. `false` only leaves that process outside
    /// the tree cleanup.
    #[must_use]
    pub fn assign(&self, process: BorrowedHandle<'_>) -> bool {
        // SAFETY: both handles are open for the call, each kept so by the
        // borrow it came from, and the call takes neither's ownership.
        unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), process.as_raw_handle()) != 0 }
    }
}

/// Whether the calling process is already in a job whose members' children
/// silently break away from it.
///
/// Nesting pnpm's own job inside such a job would pin every descendant in
/// the outer one until its owner exits, whether or not the command
/// succeeded. Where this is `true`, pnpm stays out of its own job and
/// assigns the children it spawns instead: they break away from the outer
/// job and belong to pnpm's alone.
#[must_use]
pub fn enclosing_job_releases_children() -> bool {
    let mut in_job = 0;
    // SAFETY: `GetCurrentProcess` returns the always-valid current-process
    // pseudo handle, a null job handle asks about the process's immediate
    // job, and the output is a fully initialized stack local that outlives
    // the call.
    let queried =
        unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &raw mut in_job) };
    if queried == 0 || in_job == 0 {
        return false;
    }
    limits(std::ptr::null_mut())
        .is_some_and(|flags| flags & JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK != 0)
}

fn create() -> Option<OwnedHandle> {
    // SAFETY: both arguments are null, which asks for an unnamed job with
    // default security, and the call returns a handle this call owns.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return None;
    }
    // SAFETY: the handle is non-null, newly created, and owned by this
    // call, so no other owner exists to close it.
    Some(unsafe { OwnedHandle::from_raw_handle(job) })
}

fn set_limits(job: HANDLE, flags: u32) -> bool {
    // SAFETY: `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` holds nothing but
    // integers, so an all-zero value is a valid one to build on, and it has
    // reserved fields a literal cannot name.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = flags;
    let size = size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
    // SAFETY: the handle is open for the call, kept so by the caller's
    // ownership of it, and the pointer is to a fully initialized stack
    // local of exactly the structure
    // `JobObjectExtendedLimitInformation` selects, whose size is the one
    // passed, and which outlives the call.
    unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&info).cast(),
            size,
        ) != 0
    }
}

/// The limit flags of `job`, where a null handle asks about the calling
/// process's immediate job.
fn limits(job: HANDLE) -> Option<u32> {
    // SAFETY: as in `set_limits`, an all-zero value is a valid one to read
    // into and the struct's reserved fields cannot be named by a literal.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    let size = size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
    // SAFETY: the pointer is to a fully initialized stack local of exactly
    // the structure the class selects, whose size is the one passed, and
    // which outlives the call. A null returned-length pointer discards that
    // output, which the call documents as allowed.
    let queried = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            std::ptr::from_mut(&mut info).cast(),
            size,
            std::ptr::null_mut(),
        )
    };
    (queried != 0).then_some(info.BasicLimitInformation.LimitFlags)
}
