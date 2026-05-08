# @pnpm/store.index

## 1100.1.0

### Minor Changes

- 0c67cb5: Export `pickStoreIndexKey(resolution, pkgId, { built })` — picks the appropriate store-index key for a resolution:
  git-hosted entries route through `gitHostedStoreIndexKey(pkgId, { built })`, everything else through
  `storeIndexKey(resolution.integrity, pkgId)`. Centralizes the routing for `installing.package-requester`,
  `building.after-install`, `store.pkg-finder`, and `modules-mounter.daemon` so each consumer reads
  `resolution.gitHosted` once via a single typed call.

## 1000.0.0

### Minor Changes

- b7f0f21: Use SQLite for storing package index in the content-addressable store. Instead of individual `.mpk` files under `$STORE/index/`, package metadata is now stored in a single SQLite database at `$STORE/index.db`. This reduces filesystem syscall overhead, improves space efficiency for small metadata entries, and enables concurrent access via SQLite's WAL mode. Packages missing from the new index are re-fetched on demand [#10826](https://github.com/pnpm/pnpm/issues/10826).
