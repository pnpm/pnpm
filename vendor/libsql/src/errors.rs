#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Failed to connect to database: `{0}`")]
    ConnectionFailed(String),
    #[error("SQLite failure: `{1}`")]
    SqliteFailure(std::ffi::c_int, String),
    #[error("Null value")]
    NullValue, // Not in rusqlite
    #[error("API misuse: `{0}`")]
    Misuse(String), // Not in rusqlite
    #[error("Execute returned rows")]
    ExecuteReturnedRows,
    #[error("Query returned no rows")]
    QueryReturnedNoRows,
    #[error("Invalid column name: `{0}`")]
    InvalidColumnName(String),
    #[error("SQL conversion failure: `{0}`")]
    ToSqlConversionFailure(crate::BoxError),
    #[error("Sync is not supported in databases opened in {0} mode.")]
    SyncNotSupported(String), // Not in rusqlite
    #[error("Loading extension is only supported in local databases.")]
    LoadExtensionNotSupported, // Not in rusqlite
    #[error("Authorizer is only supported in local databases.")]
    AuthorizerNotSupported, // Not in rusqlite
    #[error("Update hooks are only supported in local databases.")]
    UpdateHookNotSupported, // Not in rusqlite
    #[error("Column not found: {0}")]
    ColumnNotFound(i32), // Not in rusqlite
    #[error("Hrana: `{0}`")]
    Hrana(crate::BoxError), // Not in rusqlite
    #[error("Write delegation: `{0}`")]
    WriteDelegation(crate::BoxError), // Not in rusqlite
    #[error("bincode: `{0}`")]
    Bincode(crate::BoxError),
    #[error("invalid column index")]
    InvalidColumnIndex,
    #[error("invalid column type")]
    InvalidColumnType,
    #[error("syntax error around L{0}:{1}: `{2}`")]
    Sqlite3SyntaxError(u64, usize, String),
    #[error("unsupported statement")]
    Sqlite3UnsupportedStatement,
    #[error("sqlite3 parser error: `{0}`")]
    Sqlite3ParserError(crate::BoxError),
    #[error("Remote SQlite failure: `{0}:{1}:{2}`")]
    RemoteSqliteFailure(i32, i32, String),
    #[error("replication error: {0}")]
    Replication(crate::BoxError),
    #[error("path has invalid UTF-8")]
    InvalidUTF8Path,
    #[error("freeze is not supported in {0} mode.")]
    FreezeNotSupported(String),
    #[error("connection has reached an invalid state, started with {0}")]
    InvalidParserState(String),
    #[error("TLS error: {0}")]
    InvalidTlsConfiguration(std::io::Error),
    #[error("Transactional batch error: {0}")]
    TransactionalBatchError(String),
    #[error("Invalid blob size, expected {0}")]
    InvalidBlobSize(usize),
    #[error("sync error: {0}")]
    Sync(crate::BoxError),
    #[error("WAL frame insert conflict")]
    WalConflict,
    #[error("Reserved bytes not supported")]
    ReservedBytesNotSupported,
}

#[cfg(feature = "hrana")]
impl From<crate::hrana::HranaError> for Error {
    fn from(e: crate::hrana::HranaError) -> Self {
        Error::Hrana(e.into())
    }
}

#[cfg(feature = "sync")]
impl From<crate::sync::SyncError> for Error {
    fn from(e: crate::sync::SyncError) -> Self {
        Error::Sync(e.into())
    }
}

impl From<std::convert::Infallible> for Error {
    fn from(_: std::convert::Infallible) -> Self {
        unreachable!()
    }
}

#[cfg(feature = "core")]
pub(crate) fn error_from_handle(raw: *mut libsql_sys::ffi::sqlite3) -> String {
    let errmsg = unsafe { libsql_sys::ffi::sqlite3_errmsg(raw) };
    sqlite_errmsg_to_string(errmsg)
}

#[cfg(feature = "core")]
pub(crate) fn extended_error_code(raw: *mut libsql_sys::ffi::sqlite3) -> std::ffi::c_int {
    unsafe { libsql_sys::ffi::sqlite3_extended_errcode(raw) }
}

#[cfg(feature = "core")]
pub fn error_from_code(code: i32) -> String {
    let errmsg = unsafe { libsql_sys::ffi::sqlite3_errstr(code) };
    sqlite_errmsg_to_string(errmsg)
}

#[cfg(feature = "core")]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn sqlite_errmsg_to_string(errmsg: *const std::ffi::c_char) -> String {
    let errmsg = unsafe { std::ffi::CStr::from_ptr(errmsg) }.to_bytes();
    String::from_utf8_lossy(errmsg).to_string()
}

#[cfg(feature = "replication")]
impl From<bincode::Error> for Error {
    fn from(e: bincode::Error) -> Self {
        Error::Bincode(e.into())
    }
}
