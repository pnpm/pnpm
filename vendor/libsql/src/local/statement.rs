use crate::local::rows::{MappedRows, Row};
use crate::local::{Connection, Rows};
use crate::params::Params;
use crate::{errors, Column, Error, Result, ValueRef};

use std::cell::RefCell;
use std::ffi::c_int;
use std::sync::Arc;

/// A prepared statement.
#[derive(Debug, Clone)]
pub struct Statement {
    pub(crate) conn: Connection,
    pub(crate) inner: Arc<libsql_sys::Statement>,
    sql: String,
}

impl Statement {
    pub(crate) fn finalize(&self) {
        self.inner.finalize();
    }

    pub(crate) fn prepare(
        conn: Connection,
        raw: *mut libsql_sys::ffi::sqlite3,
        sql: &str,
    ) -> Result<Statement> {
        match unsafe { libsql_sys::prepare_stmt(raw, sql) } {
            Ok(stmt) => Ok(Statement {
                conn,
                inner: Arc::new(stmt),
                sql: sql.to_string(),
            }),
            Err(libsql_sys::Error::LibError(_err)) => Err(Error::SqliteFailure(
                errors::extended_error_code(raw),
                errors::error_from_handle(raw),
            )),
            Err(err) => Err(Error::Misuse(format!(
                "Unexpected error while preparing statement: {err}"
            ))),
        }
    }

    pub fn query_map<F, T>(&self, params: &Params, f: F) -> Result<MappedRows<F>>
    where
        F: FnMut(Row) -> Result<T>,
    {
        let rows = self.query(params)?;

        Ok(MappedRows::new(rows, f))
    }

    pub fn run(&self, params: &Params) -> Result<()> {
        self.bind(params);
        let err = self.inner.step();
        match err {
            crate::ffi::SQLITE_DONE => Ok(()),
            crate::ffi::SQLITE_ROW => Ok(()),
            _ => Err(Error::SqliteFailure(
                errors::extended_error_code(self.conn.raw),
                errors::error_from_handle(self.conn.raw),
            )),
        }
    }

    pub fn query(&self, params: &Params) -> Result<Rows> {
        self.bind(params);
        let err = self.inner.step();
        Ok(Rows::new2(
            self.clone(),
            RefCell::new(Some((
                err,
                errors::extended_error_code(self.conn.raw),
                errors::error_from_handle(self.conn.raw),
            ))),
        ))
    }

    pub fn query_row(&self, params: &Params) -> Result<Row> {
        let rows = self.query(params)?;

        let row = rows.next()?.ok_or(Error::QueryReturnedNoRows)?;

        Ok(row)
    }

    pub fn bind(&self, params: &Params) {
        match params {
            Params::None => {}
            Params::Positional(params) => {
                for (i, param) in params.iter().enumerate() {
                    let i = i as i32 + 1;

                    self.bind_value(i, param.into());
                }
            }

            Params::Named(params) => {
                for (name, param) in params {
                    let i = self.inner.bind_parameter_index(name);

                    self.bind_value(i, param.into());
                }
            }
        }
    }

    pub fn parameter_count(&self) -> usize {
        self.inner.bind_parameter_count()
    }

    pub fn parameter_name(&self, index: i32) -> Option<&str> {
        self.inner.bind_parameter_name(index)
    }

    pub fn is_explain(&self) -> i32 {
        self.inner.is_explain()
    }

    pub fn readonly(&self) -> bool {
        self.inner.readonly()
    }

    pub fn execute(&self, params: &Params) -> Result<u64> {
        self.bind(params);
        let err = self.inner.step();
        match err {
            crate::ffi::SQLITE_DONE => Ok(self.conn.changes()),
            crate::ffi::SQLITE_ROW => Err(Error::ExecuteReturnedRows),
            _ => Err(Error::SqliteFailure(
                errors::extended_error_code(self.conn.raw),
                errors::error_from_handle(self.conn.raw),
            )),
        }
    }

    /// Interrupt the statement.
    pub fn interrupt(&self) -> Result<()> {
        self.inner.interrupt();
        Ok(())
    }

    /// Reset the prepared statement to initial state for reuse.
    pub fn reset(&self) {
        self.inner.reset();
    }

    pub fn bind_value(&self, i: i32, param: ValueRef<'_>) {
        match param {
            ValueRef::Null => {
                self.inner.bind_null(i);
            }
            ValueRef::Integer(value) => {
                self.inner.bind_int64(i, value);
            }
            ValueRef::Real(value) => {
                self.inner.bind_double(i, value);
            }
            ValueRef::Text(value) => {
                self.inner.bind_text(i, value);
            }
            ValueRef::Blob(value) => {
                self.inner.bind_blob(i, value);
            }
        }
    }

    pub fn get_status(&self, status: i32) -> i32 {
        self.inner.get_status(status)
    }

    pub fn value_ref(inner: &libsql_sys::Statement, col: usize) -> ValueRef<'_> {
        let raw = inner.raw_stmt;

        match inner.column_type(col as i32) {
            crate::ffi::SQLITE_NULL => ValueRef::Null,
            crate::ffi::SQLITE_INTEGER => {
                ValueRef::Integer(unsafe { crate::ffi::sqlite3_column_int64(raw, col as c_int) })
            }
            crate::ffi::SQLITE_FLOAT => {
                ValueRef::Real(unsafe { crate::ffi::sqlite3_column_double(raw, col as c_int) })
            }
            crate::ffi::SQLITE_TEXT => {
                let s = unsafe {
                    // Quoting from "Using SQLite" book:
                    // To avoid problems, an application should first extract the desired type using
                    // a sqlite3_column_xxx() function, and then call the
                    // appropriate sqlite3_column_bytes() function.
                    let text = crate::ffi::sqlite3_column_text(raw, col as c_int);
                    let len = crate::ffi::sqlite3_column_bytes(raw, col as c_int);
                    assert!(
                        !text.is_null(),
                        "unexpected SQLITE_TEXT column type with NULL data"
                    );
                    std::slice::from_raw_parts(text.cast::<u8>(), len as usize)
                };

                ValueRef::Text(s)
            }
            crate::ffi::SQLITE_BLOB => {
                let (blob, len) = unsafe {
                    (
                        crate::ffi::sqlite3_column_blob(raw, col as c_int),
                        crate::ffi::sqlite3_column_bytes(raw, col as c_int),
                    )
                };

                assert!(
                    len >= 0,
                    "unexpected negative return from sqlite3_column_bytes"
                );
                if len > 0 {
                    assert!(
                        !blob.is_null(),
                        "unexpected SQLITE_BLOB column type with NULL data"
                    );
                    ValueRef::Blob(unsafe {
                        std::slice::from_raw_parts(blob.cast::<u8>(), len as usize)
                    })
                } else {
                    // The return value from sqlite3_column_blob() for a zero-length BLOB
                    // is a NULL pointer.
                    ValueRef::Blob(&[])
                }
            }
            _ => unreachable!("sqlite3_column_type returned invalid value"),
        }
    }

    /// Returns true if this statement has rows ready to be read.
    pub(crate) fn step(&self) -> Result<bool> {
        let err = self.inner.step();
        match err {
            crate::ffi::SQLITE_DONE => Ok(false),
            crate::ffi::SQLITE_ROW => Ok(true),
            _ => Err(Error::SqliteFailure(
                errors::extended_error_code(self.conn.raw),
                errors::error_from_handle(self.conn.raw),
            )),
        }
    }

    pub(crate) fn tail(&self) -> usize {
        self.inner.tail()
    }

    pub(crate) fn is_readonly(&self) -> bool {
        self.inner.readonly()
    }
}

impl Statement {
    /// Get all the column names in the result set of the prepared statement.
    ///
    /// If associated DB schema can be altered concurrently, you should make
    /// sure that current statement has already been stepped once before
    /// calling this method.
    pub fn column_names(&self) -> Vec<&str> {
       let n = self.column_count();
       let mut cols = Vec::with_capacity(n);
       for i in 0..n {
           let s = self.column_name(i);
           if let Some(s) = s {
               cols.push(s);
           }
       }
       cols
    }

    /// Return the number of columns in the result set returned by the prepared
    /// statement.
    ///
    /// If associated DB schema can be altered concurrently, you should make
    /// sure that current statement has already been stepped once before
    /// calling this method.
    pub fn column_count(&self) -> usize {
        self.inner.column_count() as usize
    }

    /// Returns the name assigned to a particular column in the result set
    /// returned by the prepared statement.
    ///
    /// If associated DB schema can be altered concurrently, you should make
    /// sure that current statement has already been stepped once before
    /// calling this method.
    ///
    /// Returns `None` if there is no column at the provided index.
    pub fn column_name(&self, col: usize) -> Option<&str> {
        self.inner.column_name(col as i32)
    }

    pub fn column_origin_name(&self, col: usize) -> Option<&str> {
        self.inner.column_origin_name(col as i32)
    }

    pub fn column_table_name(&self, col: usize) -> Option<&str> {
        self.inner.column_table_name(col as i32)
    }

    pub fn column_database_name(&self, col: usize) -> Option<&str> {
        self.inner.column_database_name(col as i32)
    }

    pub fn column_decltype(&self, col: usize) -> Option<&str> {
        self.inner.column_decltype(col as i32)
    }

    /// Returns the column index in the result set for a given column name.
    ///
    /// If there is no AS clause then the name of the column is unspecified and
    /// may change from one release of SQLite to the next.
    ///
    /// If associated DB schema can be altered concurrently, you should make
    /// sure that current statement has already been stepped once before
    /// calling this method.
    ///
    /// # Failure
    ///
    /// Will return an `Error::InvalidColumnName` when there is no column with
    /// the specified `name`.
    pub fn column_index(&self, name: &str) -> Result<usize> {
        let bytes = name.as_bytes();
        let n = self.column_count();
        for i in 0..n {
            // Note: `column_name` is only fallible if `i` is out of bounds,
            // which we've already checked.
            let col_name = self
                .column_name(i)
                .ok_or_else(|| Error::InvalidColumnName(name.to_string()))?;
            if bytes.eq_ignore_ascii_case(col_name.as_bytes()) {
                return Ok(i as usize);
            }
        }
        Err(Error::InvalidColumnName(name.to_string()))
    }

    /// Returns a slice describing the columns of the result of the query.
    ///
    /// If associated DB schema can be altered concurrently, you should make
    /// sure that current statement has already been stepped once before
    /// calling this method.
    pub fn columns(&self) -> Vec<Column> {
        let n = self.column_count();
        let mut cols = Vec::with_capacity(n);
        for i in 0..n {
            let name = self.column_name(i).expect("Column idx should be valid");
            let origin_name = self.column_origin_name(i);
            let table_name = self.column_table_name(i);
            let database_name = self.column_database_name(i);
            let decl_type = self.column_decltype(i);
            cols.push(Column {
                name,
                origin_name,
                table_name,
                database_name,
                decl_type,
            });
        }
        cols
    }
}
