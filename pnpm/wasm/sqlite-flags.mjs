// How libsql-ffi compiles SQLite for wasm32-wasip1-threads. Its build script
// adds WASI flags only for the `wasm32-wasi` target, so these supply what
// rusqlite's bundled build added for every WASI target: the emulated POSIX
// APIs the Unix VFS calls, and a `double` long double. The store index is
// shared between threads, so SQLite is built threadsafe.
export const SQLITE_FLAGS = [
  '-USQLITE_THREADSAFE',
  '-DSQLITE_THREADSAFE=1',
  '-DLONGDOUBLE_TYPE=double',
  '-D_WASI_EMULATED_MMAN',
  '-D_WASI_EMULATED_GETPID',
  '-D_WASI_EMULATED_SIGNAL',
  '-D_WASI_EMULATED_PROCESS_CLOCKS',
].join(' ')
