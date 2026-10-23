// TODO: Remove this once we have decided what we want to keep
// from the old api.
#![allow(dead_code)]

pub mod connection;
pub mod database;
pub mod rows;
pub mod statement;
pub mod transaction;

pub(crate) mod impls;

pub use libsql_sys::ffi;

pub use crate::{Error, Result};
pub use connection::Connection;
pub use database::Database;
pub use rows::Row;
pub use rows::Rows;
pub use rows::RowsFuture;
pub use statement::Statement;
pub use transaction::Transaction;

/// Return the version of the underlying SQLite library as a number.
pub fn version_number() -> i32 {
    unsafe { ffi::sqlite3_libversion_number() }
}

/// Return the version of the underlying SQLite library as a string.
pub fn version() -> &'static str {
    unsafe {
        std::ffi::CStr::from_ptr(ffi::sqlite3_libversion())
            .to_str()
            .unwrap()
    }
}
