# @pnpm/store.index

> SQLite-backed index for the pnpm content-addressable store

## Why SQLite instead of individual index files?

Previously, pnpm stored package metadata as individual JSON files under
`$STORE/index/`. Each resolved package had its own file, keyed by its integrity
hash. This worked but had several downsides at scale:

- **Filesystem overhead.** Every lookup required `open` / `read` / `close`
  syscalls, and every write needed an atomic `write` + `rename` per entry.
  On repositories with thousands of dependencies the accumulated I/O was
  significant.
- **Space inefficiency.** Small metadata entries still consumed a minimum
  filesystem block each (typically 4 KiB), wasting space.
Storing all entries in a single SQLite database (`$STORE/index.db`) addresses
these issues:

- **Fewer syscalls.** Reads and writes go through SQLite's page cache and
  memory-mapped I/O instead of individual file operations.
- **Space efficiency.** Small entries share database pages instead of each
  occupying a full filesystem block.
- **Batch writes.** Multiple entries can be inserted in a single transaction,
  reducing disk flushes.

## Concurrent access

`StoreIndex` and `ReadOnlyStoreIndex` coordinate access through SQLite WAL locking.
Use `ReadOnlyStoreIndex` to read a store while other connections write to it.
It observes committed updates and may create SQLite sidecar files in the store
directory. All processes must use the same local filesystem on the same host;
sharing the live WAL database across hosts over a network filesystem is unsupported.

Use `ImmutableStoreIndex` only for a finalized store that no process will change,
such as a store shipped in a read-only image. It creates no sidecar files and is
used by `frozenStore`. Callers that used `ReadOnlyStoreIndex` for finalized
stores on read-only filesystems must switch to `ImmutableStoreIndex`.

## Incomplete `node:sqlite`

When `DatabaseSync.exec` is missing, the store index runs its SQL through prepared statements. When `DatabaseSync.prepare` is missing too, entries are stored in `index.fallback`. A later pnpm with a complete `node:sqlite` reads `index.db` only. `frozenStore` still requires a connection that can open `index.db`. The file index does not coordinate concurrent writers.

## License

MIT
