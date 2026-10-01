// rusqlite 0.40 added a reference to `sqlite3_set_errmsg` (added in SQLite 3.48).
// When `backend-libsql` is enabled, `libsql-ffi` provides an older SQLite fork (~3.45)
// that lacks `sqlite3_set_errmsg`. If unresolved, linkers pull in `sqlite3.o` from
// `libsqlite3-sys` alongside `libsql-ffi`'s `sqlite3.o`, colliding on all SQLite symbols.
// Providing this symbol satisfies rusqlite's unused public error helper without loading
// the second SQLite object file.
/// Stub for `SQLite`'s [`sqlite3_set_errmsg`] to prevent duplicate-symbol collisions
/// when `backend-libsql` is enabled alongside `rusqlite 0.40`.
///
/// # Safety
///
/// This is a no-op C-ABI function that ignores its pointer arguments and always returns 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sqlite3_set_errmsg(
    _db: *mut std::ffi::c_void,
    _errcode: std::ffi::c_int,
    _msg: *const std::ffi::c_char,
) -> std::ffi::c_int {
    0
}
