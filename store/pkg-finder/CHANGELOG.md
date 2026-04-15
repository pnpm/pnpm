# @pnpm/store.pkg-finder

## 1100.0.1

### Patch Changes

- @pnpm/deps.path@1100.0.1
- @pnpm/fetching.directory-fetcher@1100.0.1
- @pnpm/resolving.resolver-base@1100.0.1
- @pnpm/store.cafs@1100.0.1

## 1000.0.0

### Major Changes

- f92ac24: Initial release.

### Minor Changes

- b7f0f21: Use SQLite for storing package index in the content-addressable store. Instead of individual `.mpk` files under `$STORE/index/`, package metadata is now stored in a single SQLite database at `$STORE/index.db`. This reduces filesystem syscall overhead, improves space efficiency for small metadata entries, and enables concurrent access via SQLite's WAL mode. Packages missing from the new index are re-fetched on demand [#10826](https://github.com/pnpm/pnpm/issues/10826).

### Patch Changes

- Updated dependencies [5f73b0f]
- Updated dependencies [facdd71]
- Updated dependencies [e2e0a32]
- Updated dependencies [9b0a460]
- Updated dependencies [3bf5e21]
- Updated dependencies [491a84f]
- Updated dependencies [6656baa]
- Updated dependencies [2ea6463]
- Updated dependencies [50fbeca]
- Updated dependencies [caabba4]
- Updated dependencies [878a773]
- Updated dependencies [f8e6774]
- Updated dependencies [7d2fd48]
- Updated dependencies [56a59df]
- Updated dependencies [50fbeca]
- Updated dependencies [10bc391]
- Updated dependencies [38b8e35]
- Updated dependencies [b7f0f21]
- Updated dependencies [1e6de25]
- Updated dependencies [9d3f00b]
  - @pnpm/deps.path@1002.0.0
  - @pnpm/resolving.resolver-base@1006.0.0
  - @pnpm/store.cafs@1001.0.0
  - @pnpm/fetching.directory-fetcher@1001.0.0
  - @pnpm/store.index@1000.0.0
