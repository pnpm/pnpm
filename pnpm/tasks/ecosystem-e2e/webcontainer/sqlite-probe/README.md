# SQLite WASI storage probe

This probe compiles the SQLite source selected by the repository's `Cargo.lock`.
Run `pnpm install` first to populate `.pnpm/crates/crates-io`.
Extract an official [WASI SDK](https://github.com/WebAssembly/wasi-sdk/releases)
and set `WASI_SDK_PATH` to its directory. SDK 34 was used for validation.

From the repository root:

```sh
WASI_SDK_PATH=/path/to/wasi-sdk node pnpm/tasks/ecosystem-e2e/webcontainer/sqlite-probe/build.mjs
node --test pnpm/tasks/ecosystem-e2e/webcontainer/sqlite-probe/test.mjs
```

The build produces `target/wasm-sqlite-probe/sqlite-probe.wasm`. Its command-line
arguments are a database path and `write`, `read`, or `hold`. `hold` keeps a
write transaction open for three seconds to exercise contention or process exit.
The runner accepts `PNPM_SQLITE_PROBE_WASM` to select another artifact.

The bundled SQLite's default WASI VFS uses a lock directory and rollback
journals. It excludes concurrent writers and retains database integrity, but a
killed process leaves the lock directory behind. The crash test waits for the
holder to exit before explicitly removing its fixture lock and checking rollback.
The Rust runtime adds a host process lease before opening SQLite. The separate
`pnpm/wasm/store.test.mjs` exercises that integration: another process waits,
the holder dies during a transaction, and the next connection automatically
recovers both the abandoned lock directory and SQLite rollback journal.

The host registry serializes only ownership-record updates through a TCP
rendezvous mutex. Active leases use ephemeral listening ports and random tokens,
so unrelated locks never share a lifetime lease. Ownership records live in a
private temporary directory. A refused connection or explicitly released token
permits recovery; slow or ambiguous responses retain the lock. File-lock waits
expire after 30 seconds, including a nested command blocked by its parent's
open store connection.
Do not enable SQLite's optional demo WASI VFS, which omits locking, or share a
live database with a native SQLite process using POSIX locks.
