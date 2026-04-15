# @pnpm/store.commands

## 1100.0.1

### Patch Changes

- b989a4a: Fixed `pnpm store prune` removing packages used by the globally installed pnpm, breaking it.
- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/cli.utils@1100.0.1
  - @pnpm/config.normalize-registries@1100.0.1
  - @pnpm/config.reader@1100.0.1
  - @pnpm/deps.path@1100.0.1
  - @pnpm/global.packages@1100.0.1
  - @pnpm/installing.client@1100.0.1
  - @pnpm/installing.context@1100.0.1
  - @pnpm/lockfile.types@1100.0.1
  - @pnpm/lockfile.utils@1100.0.1
  - @pnpm/store.cafs@1100.0.1
  - @pnpm/store.controller-types@1100.0.1
  - @pnpm/store.connection-manager@1100.0.1

## 1001.0.0

### Major Changes

- e2e0a32: Optimized index file format to store the hash algorithm once per file instead of repeating it for every file entry. Each file entry now stores only the hex digest instead of the full integrity string (`<algo>-<digest>`). Using hex format improves performance since file paths in the content-addressable store use hex representation, eliminating base64-to-hex conversion during path lookups.
- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- b7f0f21: Use SQLite for storing package index in the content-addressable store. Instead of individual `.mpk` files under `$STORE/index/`, package metadata is now stored in a single SQLite database at `$STORE/index.db`. This reduces filesystem syscall overhead, improves space efficiency for small metadata entries, and enables concurrent access via SQLite's WAL mode. Packages missing from the new index are re-fetched on demand [#10826](https://github.com/pnpm/pnpm/issues/10826).

### Patch Changes

- 5f5f1db: Fix `pnpm store path` and `pnpm store status` using workspace root for path resolution when `storeDir` is relative [#10290](https://github.com/pnpm/pnpm/issues/10290).
- b773199: `pnpm store prune` should not fail if the dlx cache directory has files, not only directories [#10384](https://github.com/pnpm/pnpm/pull/10384)
- Updated dependencies [7730a7f]
- Updated dependencies [5f73b0f]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [e2e0a32]
- Updated dependencies [3c72b6b]
- Updated dependencies [5d130c3]
- Updated dependencies [9f5c0e3]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [90bd3c3]
- Updated dependencies [1cc61e8]
- Updated dependencies [606f53e]
- Updated dependencies [c7203b9]
- Updated dependencies [bb17724]
- Updated dependencies [da2429d]
- Updated dependencies [1cc61e8]
- Updated dependencies [3bf5e21]
- Updated dependencies [491a84f]
- Updated dependencies [6656baa]
- Updated dependencies [f0ae1b9]
- Updated dependencies [2ea6463]
- Updated dependencies [50fbeca]
- Updated dependencies [caabba4]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [9eddabb]
- Updated dependencies [075aa99]
- Updated dependencies [fd511e4]
- Updated dependencies [ae43ac7]
- Updated dependencies [ccec8e7]
- Updated dependencies [ba065f6]
- Updated dependencies [4158906]
- Updated dependencies [ac944ef]
- Updated dependencies [878a773]
- Updated dependencies [f8e6774]
- Updated dependencies [e2e0a32]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
- Updated dependencies [56a59df]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [96704a1]
- Updated dependencies [50fbeca]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [38b8e35]
- Updated dependencies [394d88c]
- Updated dependencies [b7f0f21]
- Updated dependencies [1e6de25]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [ed1a7fe]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [5bf7768]
- Updated dependencies [ae43ac7]
- Updated dependencies [a5fdbf9]
- Updated dependencies [9d3f00b]
- Updated dependencies [98a0410]
- Updated dependencies [efb48dc]
- Updated dependencies [9587dac]
- Updated dependencies [09a999a]
- Updated dependencies [559f903]
- Updated dependencies [3574905]
  - @pnpm/config.reader@1005.0.0
  - @pnpm/deps.path@1002.0.0
  - @pnpm/store.controller-types@1005.0.0
  - @pnpm/store.cafs@1001.0.0
  - @pnpm/installing.context@1002.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/lockfile.types@1003.0.0
  - @pnpm/lockfile.utils@1004.0.0
  - @pnpm/cli.utils@1002.0.0
  - @pnpm/resolving.parse-wanted-dependency@1002.0.0
  - @pnpm/store.connection-manager@1003.0.0
  - @pnpm/config.normalize-registries@1001.0.0
  - @pnpm/object.key-sorting@1001.0.0
  - @pnpm/installing.client@1002.0.0
  - @pnpm/store.path@1001.0.0
  - @pnpm/fs.graceful-fs@1001.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/global.packages@1000.0.0
  - @pnpm/crypto.integrity@1100.0.0
  - @pnpm/store.index@1000.0.0
