//! In a binary of its own: the `SQLite` log callback must be installed before
//! any test in the process opens a database.
#![cfg(feature = "backend-libsql")]

use pnpm_testing_utils::sqlite_misuse::counting_misuse_reports;

/// A second `sqlite3_close_v2` on a freed handle reads freed memory, which
/// aborted the libsql tests on Windows with `0xc0000005`. `SQLite` reports a
/// close of an already closed handle as misuse.
#[tokio::test]
async fn a_dropped_connection_is_closed_once() {
    let reports =
        counting_misuse_reports().expect("no database is open yet, so SQLite accepts the callback");

    let directory = tempfile::tempdir().unwrap();
    let db = libsql::Builder::new_local(directory.path().join("auth.db")).build().await.unwrap();
    let conn = db.connect().unwrap();
    conn.execute("CREATE TABLE t (x INTEGER)", ()).await
        .unwrap();
    drop(conn);

    assert_eq!(reports.count(), 0);
}
