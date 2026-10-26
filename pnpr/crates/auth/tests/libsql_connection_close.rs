//! In a binary of its own: the `SQLite` log callback must be installed before
//! any test in the process opens a database.
#![cfg(feature = "backend-libsql")]

use libsql::ffi;
use std::{
    ffi::{c_char, c_int, c_void},
    sync::atomic::{AtomicUsize, Ordering},
};

static MISUSES: AtomicUsize = AtomicUsize::new(0);

extern "C" fn count_misuse(_: *mut c_void, code: c_int, _: *const c_char) {
    if code & 0xff == ffi::SQLITE_MISUSE {
        MISUSES.fetch_add(1, Ordering::SeqCst);
    }
}

/// A second `sqlite3_close_v2` on a freed handle reads freed memory, which
/// aborted the libsql tests on Windows with `0xc0000005`. `SQLite` reports a
/// close of an already closed handle as misuse.
#[tokio::test]
async fn a_dropped_connection_is_closed_once() {
    let log: extern "C" fn(*mut c_void, c_int, *const c_char) = count_misuse;
    // SAFETY: no database is open yet, so `SQLite` is not initialized and
    // accepts configuration.
    let configured =
        unsafe { ffi::sqlite3_config(ffi::SQLITE_CONFIG_LOG, log, std::ptr::null_mut::<c_void>()) };
    assert_eq!(configured, ffi::SQLITE_OK);

    let directory = tempfile::tempdir().unwrap();
    let db = libsql::Builder::new_local(directory.path().join("auth.db")).build().await.unwrap();
    let conn = db.connect().unwrap();
    conn.execute("CREATE TABLE t (x INTEGER)", ()).await
        .unwrap();
    drop(conn);

    assert_eq!(MISUSES.load(Ordering::SeqCst), 0);
}
