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

## License

MIT
