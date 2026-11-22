use std::rc::Rc;

use tokio::process::Child;

/// Windows does not have a concept of parent processes and so
/// killing the deno task process will not also kill any spawned
/// processes by default. To make this work, we can use winapi's
/// jobs api, which allows for associating a main process so that
/// when the main process terminates, it will also terminate the
/// associated processes.
///
/// Read more: https://stackoverflow.com/questions/3342941/kill-child-process-when-parent-process-is-killed
#[derive(Clone)]
pub struct ChildProcessTracker(Rc<dyn Tracker>);

impl ChildProcessTracker {
    #[cfg(windows)]
    pub fn new() -> Self {
        match windows::JobObject::new() {
            Ok(tracker) => Self(Rc::new(tracker)),
            Err(err) => {
                if cfg!(debug_assertions) {
                    panic!("Could not start tracking processes. {}", err);
                } else {
                    // fallback to not tracking processes if this fails
                    Self(Rc::new(NullChildProcessTracker))
                }
            }
        }
    }

    #[cfg(not(windows))]
    pub fn new() -> Self {
        // no-op on non-windows platforms as they don't
        // require tracking the child processes
        Self(Rc::new(NullChildProcessTracker))
    }

    pub fn track(&self, child: &Child) {
        if let Err(err) = self.0.track(child)
            && cfg!(debug_assertions)
            && child.id().is_some()
        {
            panic!("Could not track process: {}", err);
        }
    }
}

trait Tracker: Send + Sync {
    fn track(&self, child: &Child) -> std::io::Result<()>;
}

struct NullChildProcessTracker;

impl Tracker for NullChildProcessTracker {
    fn track(&self, _: &Child) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::ptr;

    use tokio::process::Child;

    use windows_sys::Win32::{
        Foundation::{
            HANDLE,
            INVALID_HANDLE_VALUE,
            TRUE,
        },
        System::JobObjects::{
            AssignProcessToJobObject,
            CreateJobObjectW,
            JOB_OBJECT_LIMIT_BREAKAWAY_OK,
            JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
    };

    use super::Tracker;

    pub struct JobObject(WinHandle);

    impl JobObject {
        pub fn new() -> std::io::Result<Self> {
            // SAFETY: WinAPI calls
            unsafe {
                let handle = WinHandle::new(CreateJobObjectW(ptr::null_mut(), ptr::null()));
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                    | JOB_OBJECT_LIMIT_BREAKAWAY_OK
                    | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK
                    | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
                let result = SetInformationJobObject(
                    handle.as_raw(),
                    JobObjectExtendedLimitInformation,
                    &mut info as *mut _ as *mut core::ffi::c_void,
                    std::mem::size_of_val(&info) as u32,
                );
                if result != TRUE {
                    return Err(std::io::Error::last_os_error());
                }

                Ok(Self(handle))
            }
        }

        fn add_process_handle(&self, process_handle: HANDLE) -> std::io::Result<()> {
            // SAFETY: WinAPI call
            unsafe {
                let result = AssignProcessToJobObject(self.0.as_raw(), process_handle);
                if result != TRUE { Err(std::io::Error::last_os_error()) } else { Ok(()) }
            }
        }
    }

    impl Tracker for JobObject {
        fn track(&self, child: &Child) -> std::io::Result<()> {
            if let Some(handle) = child.raw_handle() {
                self.add_process_handle(handle)
            } else {
                // process exited... ignore
                Ok(())
            }
        }
    }

    struct WinHandle(HANDLE);

    impl WinHandle {
        pub fn new(handle: HANDLE) -> Self {
            WinHandle(handle)
        }

        pub fn as_raw(&self) -> HANDLE {
            self.0
        }
    }

    unsafe impl Send for WinHandle {}
    unsafe impl Sync for WinHandle {}

    impl Drop for WinHandle {
        fn drop(&mut self) {
            // SAFETY: WinAPI calls
            unsafe {
                if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                    windows_sys::Win32::Foundation::CloseHandle(self.0);
                }
            }
        }
    }
}
