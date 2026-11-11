//! Counting the misuse `SQLite` reports, from a test.
//!
//! The foreign call a test needs to watch `SQLite`'s own error log, which
//! reports a close of an already closed handle as misuse. One log callback
//! per process, and `SQLite` only accepts one before it initializes, so a
//! caller holds its test to a file of its own and installs before anything
//! opens a database.
//!
//! The callback goes through `libsql`'s bindings because the workspace links
//! two `SQLite` libraries, `libsql-ffi` and `libsqlite3-sys`. Configuring one
//! says nothing about the other.

use libsql::ffi;
use std::{
    ffi::{c_char, c_int, c_void},
    sync::atomic::{AtomicUsize, Ordering},
};

static REPORTS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn count(_: *mut c_void, code: c_int, _: *const c_char) {
    if code & 0xff == ffi::SQLITE_MISUSE {
        REPORTS.fetch_add(1, Ordering::SeqCst);
    }
}

/// How many misuse reports `SQLite` has logged since the counter was
/// installed.
pub struct MisuseReports;

impl MisuseReports {
    /// The count so far.
    #[must_use]
    pub fn count(&self) -> usize {
        REPORTS.load(Ordering::SeqCst)
    }
}

/// Install the counter, reporting `None` when `SQLite` refuses the
/// configuration because it is already initialized.
#[must_use]
pub fn counting_misuse_reports() -> Option<MisuseReports> {
    let callback: extern "C" fn(*mut c_void, c_int, *const c_char) = count;
    // SAFETY: `sqlite3_config` takes the option by value and the callback as
    // a function pointer that lives for the program. The null third argument
    // is the context passed back to the callback, which ignores it. The
    // return value says whether the library accepted the call, which it
    // only does before it initializes.
    let configured = unsafe {
        ffi::sqlite3_config(ffi::SQLITE_CONFIG_LOG, callback, std::ptr::null_mut::<c_void>())
    };
    (configured == ffi::SQLITE_OK).then_some(MisuseReports)
}
