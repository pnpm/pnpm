## 12.13.0

### Minor Changes

- Added the `storeIndexFormat` setting. Set it to `files` to keep a store's package index as one file per package instead of the SQLite `index.db`, for a store on a network filesystem that several machines use at once. SQLite's locking does not work across machines, so concurrent installs on such a store could corrupt `index.db`.
