#![allow(dead_code)]

use crate::auth::{AuthAction, AuthContext, Authorization};
use crate::connection::AuthHook;
use crate::local::rows::BatchedRows;
use crate::params::Params;
use crate::{
    connection::{BatchRows, Op, UpdateHook},
    errors,
};
use std::time::Duration;

use super::{Database, Error, Result, Rows, RowsFuture, Statement, Transaction};

use crate::TransactionBehavior;

use libsql_sys::ffi;
use parking_lot::RwLock;
use std::{ffi::c_int, fmt, path::Path, sync::Arc};

struct Container {
    cb: Box<UpdateHook>,
}

/// A connection to a libSQL database.
#[derive(Clone)]
pub struct Connection {
    pub(crate) raw: *mut ffi::sqlite3,

    drop_ref: Arc<()>,

    #[cfg(feature = "replication")]
    pub(crate) writer: Option<crate::replication::Writer>,

    authorizer: Arc<RwLock<Option<AuthHook>>>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.disconnect()
    }
}

// SAFETY: This is safe because we compile sqlite3 w/ SQLITE_THREADSAFE=1
unsafe impl Send for Connection {}
// SAFETY: This is safe because we compile sqlite3 w/ SQLITE_THREADSAFE=1
unsafe impl Sync for Connection {}

impl Connection {
    /// Connect to the database.
    pub(crate) fn connect(db: &Database) -> Result<Connection> {
        let mut raw = std::ptr::null_mut();
        let db_path = db.db_path.clone();
        let err = unsafe {
            ffi::sqlite3_open_v2(
                std::ffi::CString::new(db_path.as_str())
                    .unwrap()
                    .as_c_str()
                    .as_ptr() as *const _,
                &mut raw,
                db.flags.bits() as c_int,
                std::ptr::null(),
            )
        };
        match err {
            ffi::SQLITE_OK => {}
            _ => {
                return Err(Error::ConnectionFailed(format!(
                    "Unable to open connection to local database {db_path}: {err}",
                )));
            }
        }
        let conn = Connection {
            raw,
            drop_ref: Arc::new(()),
            #[cfg(feature = "replication")]
            writer: db.writer()?,
            authorizer: Arc::new(RwLock::new(None)),
        };
        #[cfg(feature = "sync")]
        if let Some(_) = db.sync_ctx {
            // We need to make sure database is in WAL mode with checkpointing
            // disabled so that we can sync our changes back to a remote
            // server.
            conn.query("PRAGMA journal_mode = WAL", Params::None)?;
            conn.wal_disable_checkpoint()?;
        }
        Ok(conn)
    }

    /// Get a raw handle to the underlying libSQL connection
    pub fn handle(&self) -> *mut ffi::sqlite3 {
        self.raw
    }

    /// Create a connection from a raw handle to the underlying libSQL connection
    pub fn from_handle(raw: *mut ffi::sqlite3) -> Self {
        Self {
            raw,
            drop_ref: Arc::new(()),
            #[cfg(feature = "replication")]
            writer: None,
            authorizer: Arc::new(RwLock::new(None)),
        }
    }

    /// Disconnect from the database.
    pub fn disconnect(&mut self) {
        if Arc::get_mut(&mut self.drop_ref).is_some() {
            unsafe { libsql_sys::ffi::sqlite3_close_v2(self.raw) };
        }
    }

    /// Prepare the SQL statement.
    pub fn prepare<S: Into<String>>(&self, sql: S) -> Result<Statement> {
        Statement::prepare(self.clone(), self.raw, sql.into().as_str())
    }

    /// Convenience method to run a prepared statement query.
    /// ## Example
    ///
    /// ```rust,no_run,ignore
    /// # use libsql::Result;
    /// # use libsql::v1::{Connection, Rows};
    /// # fn create_tables(conn: &Connection) -> Result<Option<Rows>> {
    /// conn.query("SELECT * FROM users WHERE name = ?1;", vec![libsql::Value::from(1)])
    /// # }
    /// ```
    pub fn query<S, P>(&self, sql: S, params: P) -> Result<Option<Rows>>
    where
        S: Into<String>,
        P: TryInto<Params>,
        P::Error: Into<crate::BoxError>,
    {
        let stmt = Statement::prepare(self.clone(), self.raw, sql.into().as_str())?;
        let params = params
            .try_into()
            .map_err(|e| Error::ToSqlConversionFailure(e.into()))?;
        let ret = stmt.query(&params)?;
        Ok(Some(ret))
    }

    /// Convenience method to run multiple SQL statements (that cannot take any
    /// parameters).
    ///
    /// ## Example
    ///
    /// ```rust,no_run,ignore
    /// # use libsql::Result;
    /// # use libsql::v1::Connection;
    /// # fn create_tables(conn: &Connection) -> Result<()> {
    /// conn.execute_batch(
    ///     "BEGIN;
    ///     CREATE TABLE foo(x INTEGER);
    ///     CREATE TABLE bar(y TEXT);
    ///     COMMIT;",
    /// )
    /// # }
    /// ```
    ///
    /// # Failure
    ///
    /// Will return `Err` if `sql` cannot be converted to a C-compatible string
    /// or if the underlying SQLite call fails.
    pub fn execute_batch<S>(&self, sql: S) -> Result<BatchRows>
    where
        S: Into<String>,
    {
        let sql = sql.into();
        let mut sql = sql.as_str();

        let mut batch_rows = Vec::new();

        while !sql.is_empty() {
            let stmt = self.prepare(sql)?;

            let tail = if !stmt.inner.raw_stmt.is_null() {
                let returned_rows = stmt.step()?;

                let tail = stmt.tail();

                // Check if there are rows to be extracted, we must do this upfront due to the lazy
                // nature of sqlite and our somewhat hacked batch command.
                if returned_rows {
                    // Extract columns
                    let cols = stmt
                        .columns()
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            use crate::value::ValueType;

                            let val = stmt.inner.column_type(i as i32);
                            let t = match val {
                                libsql_sys::ffi::SQLITE_INTEGER => ValueType::Integer,
                                libsql_sys::ffi::SQLITE_FLOAT => ValueType::Real,
                                libsql_sys::ffi::SQLITE_BLOB => ValueType::Blob,
                                libsql_sys::ffi::SQLITE_TEXT => ValueType::Text,
                                libsql_sys::ffi::SQLITE_NULL => ValueType::Null,
                                _ => unreachable!("unknown column type {} at index {}", val, i),
                            };

                            (c.name.to_string(), t)
                        })
                        .collect::<Vec<_>>();

                    let mut rows = Vec::new();

                    // If returned rows we must extract the rows available right away instead of
                    // using the `Rows` type we have already. This is due to the step api once its
                    // returned SQLITE_ROWS we must extract them before we call step again.
                    {
                        let row = crate::local::Row { stmt: stmt.clone() };

                        let mut values = Vec::with_capacity(cols.len());

                        for i in 0..cols.len() {
                            let value = row.get_value(i as i32)?;

                            values.push(value);
                        }

                        rows.push(values);
                    }

                    // Now we can use the normal rows type to extract any n+1 rows
                    let rows_sys = Rows::new(stmt);

                    while let Some(row) = rows_sys.next()? {
                        let mut values = Vec::with_capacity(cols.len());

                        for i in 0..cols.len() {
                            let value = row.get_value(i as i32)?;

                            values.push(value);
                        }

                        rows.push(values);
                    }

                    rows.len();

                    batch_rows.push(Some(crate::Rows::new(BatchedRows::new(cols, rows))));
                } else {
                    batch_rows.push(None);
                }

                tail
            } else {
                stmt.tail()
            };

            if tail == 0 || tail >= sql.len() {
                break;
            }

            sql = &sql[tail..];
        }

        Ok(BatchRows::new(batch_rows))
    }

    fn execute_transactional_batch_inner<S>(&self, sql: S) -> Result<()>
    where
        S: Into<String>,
    {
        let sql = sql.into();
        let mut sql = sql.as_str();
        while !sql.is_empty() {
            let stmt = self.prepare(sql)?;

            let tail = stmt.tail();
            let stmt_sql = if tail == 0 || tail >= sql.len() {
                sql
            } else {
                &sql[..tail]
            };
            let prefix_count = stmt_sql.chars().take_while(|c| c.is_whitespace()).count();
            let stmt_sql = &stmt_sql[prefix_count..];
            if stmt_sql.starts_with("BEGIN")
                || stmt_sql.starts_with("COMMIT")
                || stmt_sql.starts_with("ROLLBACK")
                || stmt_sql.starts_with("END")
            {
                return Err(Error::TransactionalBatchError(
                    "Transactions forbidden inside transactional batch".to_string(),
                ));
            }

            if !stmt.inner.raw_stmt.is_null() {
                stmt.step()?;
            }

            if tail == 0 || tail >= sql.len() {
                break;
            }

            sql = &sql[tail..];
        }

        Ok(())
    }

    pub fn execute_transactional_batch<S>(&self, sql: S) -> Result<()>
    where
        S: Into<String>,
    {
        self.execute("BEGIN TRANSACTION", Params::None)?;

        match self.execute_transactional_batch_inner(sql) {
            Ok(_) => {
                self.execute("COMMIT", Params::None)?;
                Ok(())
            }
            Err(e) => {
                self.execute("ROLLBACK", Params::None)?;
                Err(e)
            }
        }
    }

    /// Execute the SQL statement synchronously.
    ///
    /// If you execute a SQL query statement (e.g. `SELECT` statement) that
    /// returns the number of rows changed.
    ///
    /// This method blocks the thread until the SQL statement is executed.
    pub fn execute<S, P>(&self, sql: S, params: P) -> Result<u64>
    where
        S: Into<String>,
        P: TryInto<Params>,
        P::Error: Into<crate::BoxError>,
    {
        let stmt = Statement::prepare(self.clone(), self.raw, sql.into().as_str())?;
        let params = params
            .try_into()
            .map_err(|e| Error::ToSqlConversionFailure(e.into()))?;
        stmt.execute(&params)
    }

    /// Execute the SQL statement synchronously.
    ///
    /// This method never blocks the thread until, but instead returns a
    /// `RowsFuture` object immediately that can be used to deferredly
    /// execute the statement.
    pub fn execute_async<S, P>(&self, sql: S, params: P) -> RowsFuture
    where
        S: Into<String>,
        P: Into<Params>,
    {
        RowsFuture {
            conn: self.clone(),
            sql: sql.into(),
            params: params.into(),
        }
    }

    /// Begin a new transaction in DEFERRED mode, which is the default.
    pub fn transaction(&self) -> Result<Transaction> {
        self.transaction_with_behavior(TransactionBehavior::Deferred)
    }

    /// Begin a new transaction in the given mode.
    pub fn transaction_with_behavior(
        &self,
        tx_behavior: TransactionBehavior,
    ) -> Result<Transaction> {
        Transaction::begin(self.clone(), tx_behavior)
    }

    pub fn interrupt(&self) -> Result<()> {
        unsafe { ffi::sqlite3_interrupt(self.raw) };
        Ok(())
    }

    pub fn busy_timeout(&self, timeout: Duration) -> Result<()> {
        unsafe { ffi::sqlite3_busy_timeout(self.raw, timeout.as_millis() as i32) };
        Ok(())
    }

    pub fn is_autocommit(&self) -> bool {
        unsafe { ffi::sqlite3_get_autocommit(self.raw) != 0 }
    }

    pub fn changes(&self) -> u64 {
        unsafe { ffi::sqlite3_changes64(self.raw) as u64 }
    }

    pub fn total_changes(&self) -> u64 {
        unsafe { ffi::sqlite3_total_changes(self.raw) as u64 }
    }

    pub fn last_insert_rowid(&self) -> i64 {
        unsafe { ffi::sqlite3_last_insert_rowid(self.raw) }
    }

    #[cfg(feature = "replication")]
    pub(crate) fn writer(&self) -> Option<&crate::replication::Writer> {
        self.writer.as_ref()
    }

    #[cfg(feature = "replication")]
    pub(crate) fn new_connection_writer(&self) -> Option<crate::replication::Writer> {
        self.writer.as_ref().cloned().map(|mut w| {
            w.new_client_id();
            w
        })
    }

    /// Installs update hook
    pub fn add_update_hook(&self, cb: Box<UpdateHook>) {
        let c = Box::new(Container { cb });
        let ptr: *mut Container = std::ptr::from_mut(Box::leak(c));

        let old_data = unsafe {
            ffi::sqlite3_update_hook(
                self.raw,
                Some(update_hook_cb),
                ptr as *mut ::std::os::raw::c_void,
            )
        };

        if !old_data.is_null() {
            let _ = unsafe { Box::from_raw(old_data as *mut Container) };
        }
    }

    pub fn enable_load_extension(&self, onoff: bool) -> Result<()> {
        // SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION configration verb accepts 2 additional parameters: an on/off flag and a pointer to an c_int where new state of the parameter will be written (or NULL if reporting back the setting is not needed)
        // See: https://sqlite.org/c3ref/c_dbconfig_defensive.html#sqlitedbconfigenableloadextension
        let err = unsafe {
            ffi::sqlite3_db_config(
                self.raw,
                ffi::SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION,
                onoff as i32,
                std::ptr::null::<c_int>(),
            )
        };
        match err {
            ffi::SQLITE_OK => Ok(()),
            _ => Err(errors::Error::SqliteFailure(
                err,
                errors::error_from_code(err),
            )),
        }
    }

    pub fn load_extension(&self, dylib_path: &Path, entry_point: Option<&str>) -> Result<()> {
        let mut raw_err_msg: *mut std::ffi::c_char = std::ptr::null_mut();
        let dylib_path = match dylib_path.to_str() {
            Some(dylib_path) => std::ffi::CString::new(dylib_path).unwrap(),
            None => {
                return Err(crate::Error::Misuse(format!(
                    "dylib path is not a valid utf8 string"
                )))
            }
        };
        let err = match entry_point {
            Some(entry_point) => {
                let entry_point = std::ffi::CString::new(entry_point).unwrap();
                unsafe {
                    ffi::sqlite3_load_extension(
                        self.raw,
                        dylib_path.as_ptr(),
                        entry_point.as_ptr(),
                        &mut raw_err_msg,
                    )
                }
            }
            None => unsafe {
                ffi::sqlite3_load_extension(
                    self.raw,
                    dylib_path.as_ptr(),
                    std::ptr::null(),
                    &mut raw_err_msg,
                )
            },
        };
        match err {
            ffi::SQLITE_OK => Ok(()),
            _ => {
                let err_msg = unsafe { std::ffi::CStr::from_ptr(raw_err_msg) };
                let err_msg = err_msg.to_string_lossy().to_string();
                unsafe { ffi::sqlite3_free(raw_err_msg as *mut std::ffi::c_void) };
                Err(errors::Error::SqliteFailure(err, err_msg))
            }
        }
    }

    pub fn authorizer(&self, hook: Option<AuthHook>) -> Result<()> {
        unsafe {
            let rc =
                libsql_sys::ffi::sqlite3_set_authorizer(self.handle(), None, std::ptr::null_mut());
            if rc != ffi::SQLITE_OK {
                return Err(crate::errors::Error::SqliteFailure(
                    rc as std::ffi::c_int,
                    "Failed to clear authorizer".to_string(),
                ));
            }
        }

        *self.authorizer.write() = hook.clone();

        let (callback, user_data) = match hook {
            Some(_) => {
                let callback = authorizer_callback as unsafe extern "C" fn(_, _, _, _, _, _) -> _;
                let user_data = self as *const Connection as *mut ::std::os::raw::c_void;
                (Some(callback), user_data)
            }
            None => (None, std::ptr::null_mut()),
        };

        let rc =
            unsafe { libsql_sys::ffi::sqlite3_set_authorizer(self.handle(), callback, user_data) };
        if rc != ffi::SQLITE_OK {
            return Err(crate::errors::Error::SqliteFailure(
                rc as std::ffi::c_int,
                "Failed to set authorizer".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn wal_checkpoint(&self, truncate: bool) -> Result<()> {
        let mut pn_log = 0i32;
        let mut pn_ckpt = 0i32;
        let checkpoint_mode = if truncate {
            libsql_sys::ffi::SQLITE_CHECKPOINT_TRUNCATE
        } else {
            libsql_sys::ffi::SQLITE_CHECKPOINT_PASSIVE
        };
        let rc = unsafe {
            libsql_sys::ffi::sqlite3_wal_checkpoint_v2(
                self.handle(),
                std::ptr::null(),
                checkpoint_mode,
                &mut pn_log,
                &mut pn_ckpt,
            )
        };
        if rc != 0 {
            let err_msg = unsafe { libsql_sys::ffi::sqlite3_errmsg(self.handle()) };
            let err_msg = unsafe { std::ffi::CStr::from_ptr(err_msg) };
            let err_msg = err_msg.to_string_lossy().to_string();
            return Err(crate::errors::Error::SqliteFailure(
                rc as std::ffi::c_int,
                format!("Failed to checkpoint WAL: {}", err_msg),
            ));
        }
        if truncate && (pn_log != 0 || pn_ckpt != 0) {
            return Err(crate::errors::Error::SqliteFailure(
                libsql_sys::ffi::SQLITE_ERROR,
                "unable to truncate WAL".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn wal_frame_count(&self) -> u32 {
        let mut max_frame_no: std::os::raw::c_uint = 0;
        unsafe { libsql_sys::ffi::libsql_wal_frame_count(self.handle(), &mut max_frame_no) };

        max_frame_no
    }

    pub(crate) fn wal_get_frame(&self, frame_no: u32, page_size: u32) -> Result<bytes::BytesMut> {
        use bytes::BufMut;

        let frame_size: usize = 24 + page_size as usize;

        // Use a BytesMut to provide cheaper clones of frame data (think retries)
        // and more efficient buffer usage for extracting wal frames and spliting them off.
        let mut buf = bytes::BytesMut::with_capacity(frame_size);

        if frame_no == 0 {
            return Err(errors::Error::SqliteFailure(
                1,
                "frame_no must be non-zero".to_string(),
            ));
        }

        let rc = unsafe {
            libsql_sys::ffi::libsql_wal_get_frame(
                self.handle(),
                frame_no,
                buf.chunk_mut().as_mut_ptr() as *mut _,
                frame_size as u32,
            )
        };

        if rc != 0 {
            return Err(crate::errors::Error::SqliteFailure(
                rc as std::ffi::c_int,
                format!("Failed to get frame: {}", frame_no),
            ));
        }

        unsafe { buf.advance_mut(frame_size) };

        Ok(buf)
    }

    fn wal_disable_checkpoint(&self) -> Result<()> {
        let rc = unsafe { libsql_sys::ffi::libsql_wal_disable_checkpoint(self.handle()) };
        if rc != 0 {
            return Err(crate::errors::Error::SqliteFailure(
                rc as std::ffi::c_int,
                format!("wal_disable_checkpoint failed"),
            ));
        }
        Ok(())
    }
    fn wal_insert_begin(&self) -> Result<()> {
        let rc = unsafe { libsql_sys::ffi::libsql_wal_insert_begin(self.handle()) };
        if rc != 0 {
            return Err(crate::errors::Error::SqliteFailure(
                rc as std::ffi::c_int,
                format!("wal_insert_begin failed"),
            ));
        }
        Ok(())
    }

    fn wal_insert_end(&self) -> Result<()> {
        let rc = unsafe { libsql_sys::ffi::libsql_wal_insert_end(self.handle()) };
        if rc != 0 {
            return Err(crate::errors::Error::SqliteFailure(
                rc as std::ffi::c_int,
                format!("wal_insert_end failed"),
            ));
        }
        Ok(())
    }

    fn wal_insert_frame(&self, frame_no: u32, frame: &[u8]) -> Result<()> {
        let mut conflict = 0i32;
        let rc = unsafe {
            libsql_sys::ffi::libsql_wal_insert_frame(
                self.handle(),
                frame_no,
                frame.as_ptr() as *mut std::ffi::c_void,
                frame.len() as u32,
                &mut conflict,
            )
        };

        if conflict != 0 {
            return Err(errors::Error::WalConflict);
        }
        if rc != 0 {
            return Err(errors::Error::SqliteFailure(
                rc as std::ffi::c_int,
                "wal_insert_frame failed".to_string(),
            ));
        }

        Ok(())
    }

    pub(crate) fn wal_insert_handle(&self) -> WalInsertHandle<'_> {
        WalInsertHandle {
            conn: self,
            in_session: RwLock::new(false),
        }
    }

    fn reserved_bytes(&self, reserve: Option<i32>) -> Result<i32> {
        let mut reserve_value = reserve.unwrap_or(0) as std::ffi::c_int;
        let rc = unsafe {
            ffi::sqlite3_file_control(
                self.raw,
                "main\0".as_ptr() as *const _,
                ffi::SQLITE_FCNTL_RESERVE_BYTES,
                &mut reserve_value as *mut _ as *mut std::ffi::c_void,
            )
        };
        if rc != ffi::SQLITE_OK {
            return Err(Error::SqliteFailure(
                rc,
                errors::error_from_handle(self.raw),
            ));
        }
        Ok(reserve_value as i32)
    }

    pub fn set_reserved_bytes(&self, reserved_bytes: i32) -> Result<()> {
        self.reserved_bytes(Some(reserved_bytes))?;
        Ok(())
    }

    pub fn get_reserved_bytes(&self) -> Result<i32> {
        self.reserved_bytes(None)
    }
}

unsafe extern "C" fn authorizer_callback(
    user_data: *mut ::std::os::raw::c_void,
    code: ::std::os::raw::c_int,
    arg1: *const ::std::os::raw::c_char,
    arg2: *const ::std::os::raw::c_char,
    database_name: *const ::std::os::raw::c_char,
    accessor: *const ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    let conn = user_data as *const Connection;
    let hook = unsafe { (*conn).authorizer.read() };
    let hook = match &*hook {
        Some(hook) => hook,
        None => return ffi::SQLITE_OK,
    };
    let arg1 = if arg1.is_null() {
        None
    } else {
        unsafe { std::ffi::CStr::from_ptr(arg1).to_str().ok() }
    };

    let arg2 = if arg2.is_null() {
        None
    } else {
        unsafe { std::ffi::CStr::from_ptr(arg2).to_str().ok() }
    };
    let database_name = if database_name.is_null() {
        None
    } else {
        unsafe { std::ffi::CStr::from_ptr(database_name).to_str().ok() }
    };
    let accessor = if accessor.is_null() {
        None
    } else {
        unsafe { std::ffi::CStr::from_ptr(accessor).to_str().ok() }
    };
    let action = AuthAction::from_raw(code, arg1, arg2);
    let auth_context = AuthContext {
        action,
        database_name,
        accessor,
    };
    match hook(&auth_context) {
        Authorization::Allow => ffi::SQLITE_OK,
        Authorization::Deny => ffi::SQLITE_DENY,
        Authorization::Ignore => ffi::SQLITE_IGNORE,
    }
}

pub(crate) struct WalInsertHandle<'a> {
    conn: &'a Connection,
    in_session: RwLock<bool>,
}

impl WalInsertHandle<'_> {
    pub fn insert_at(&self, frame_no: u32, frame: &[u8]) -> Result<()> {
        assert!(*self.in_session.read());
        self.conn.wal_insert_frame(frame_no, frame)
    }

    pub fn in_session(&self) -> bool {
        *self.in_session.read()
    }

    pub fn begin(&self) -> Result<()> {
        assert!(!*self.in_session.read());
        self.conn.wal_insert_begin()?;
        *self.in_session.write() = true;
        Ok(())
    }

    pub fn end(&self) -> Result<()> {
        assert!(*self.in_session.read());
        self.conn.wal_insert_end()?;
        *self.in_session.write() = false;
        Ok(())
    }
}

impl Drop for WalInsertHandle<'_> {
    fn drop(&mut self) {
        if *self.in_session.read() {
            if let Err(err) = self.conn.wal_insert_end() {
                tracing::error!("{:?}", err);
                Err(err).unwrap()
            }
        }
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection").finish()
    }
}

#[no_mangle]
extern "C" fn update_hook_cb(
    data: *mut ::std::os::raw::c_void,
    op: ::std::os::raw::c_int,
    db_name: *const ::std::os::raw::c_char,
    table_name: *const ::std::os::raw::c_char,
    row_id: i64,
) {
    let db = unsafe { std::ffi::CStr::from_ptr(db_name).to_string_lossy() };
    let table = unsafe { std::ffi::CStr::from_ptr(table_name).to_string_lossy() };

    let c = unsafe { &mut *(data as *mut Container) };
    let o = match op {
        9 => Op::Delete,
        18 => Op::Insert,
        23 => Op::Update,
        _ => unreachable!("Unknown operation {op}"),
    };

    (*c.cb)(o, &db, &table, row_id);
}

#[cfg(test)]
mod tests {
    use crate::{
        local::{Connection, Database},
        params::Params,
        OpenFlags,
    };

    #[tokio::test]
    pub async fn test_kek() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path1 = temp_dir.path().join("local1.db");
        let db1 = Database::new(path1.to_str().unwrap().to_string(), OpenFlags::default());
        let conn1 = Connection::connect(&db1).unwrap();
        conn1
            .query("PRAGMA journal_mode = WAL", Params::None)
            .unwrap();
        conn1.wal_disable_checkpoint().unwrap();

        let path2 = temp_dir.path().join("local2.db");
        let db2 = Database::new(path2.to_str().unwrap().to_string(), OpenFlags::default());
        let conn2 = Connection::connect(&db2).unwrap();
        conn2
            .query("PRAGMA journal_mode = WAL", Params::None)
            .unwrap();
        conn2.wal_disable_checkpoint().unwrap();

        conn1.execute("CREATE TABLE t(x)", Params::None).unwrap();
        const CNT: usize = 32;
        for _ in 0..CNT {
            conn1
                .execute(
                    "INSERT INTO t VALUES (randomblob(1024 * 1024))",
                    Params::None,
                )
                .unwrap();
        }
        let handle = conn2.wal_insert_handle();
        handle.begin().unwrap();

        let frame_count = conn1.wal_frame_count();
        for frame_no in 0..frame_count {
            let frame = conn1.wal_get_frame(frame_no + 1, 4096).unwrap();
            handle.insert_at(frame_no as u32 + 1, &frame).unwrap();
        }
        let result = conn2.query("SELECT COUNT(*) FROM t", Params::None).unwrap();
        let row = result.unwrap().next().unwrap().unwrap();
        let column = row.get_value(0).unwrap();
        let cnt = *column.as_integer().unwrap();
        assert_eq!(cnt, 32 as i64);
    }

    #[tokio::test]
    pub async fn test_reserved_bytes() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("local1.db");
        let reserved_bytes = 28;

        {
            let db = Database::new(db_path.to_str().unwrap().to_string(), OpenFlags::default());
            let conn = Connection::connect(&db).unwrap();
            conn.query("PRAGMA journal_mode = WAL", Params::None)
                .unwrap();
            conn.set_reserved_bytes(reserved_bytes).unwrap();
            conn.query("VACUUM", Params::None).unwrap();
            let reserved = conn.get_reserved_bytes().unwrap();
            assert_eq!(reserved, reserved_bytes);
        }

        // let's verify we can see this from another connection
        {
            let db = Database::new(db_path.to_str().unwrap().to_string(), OpenFlags::default());
            let conn = Connection::connect(&db).unwrap();
            let reserved = conn.get_reserved_bytes().unwrap();
            assert_eq!(reserved, reserved_bytes);
        }

        // lets make some inserts, checkpoint and verify again
        {
            let db = Database::new(db_path.to_str().unwrap().to_string(), OpenFlags::default());
            let conn = Connection::connect(&db).unwrap();
            conn.execute("CREATE TABLE t(x)", Params::None).unwrap();
            const CNT: usize = 8;
            for _ in 0..CNT {
                conn.execute(
                    "INSERT INTO t VALUES (randomblob(1024 * 1024))",
                    Params::None,
                )
                .unwrap();
            }
            conn.wal_checkpoint(true).unwrap();
            let reserved = conn.get_reserved_bytes().unwrap();
            assert_eq!(reserved, reserved_bytes);
        }
    }
}
