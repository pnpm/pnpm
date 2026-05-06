# @pnpm/store.commands

## 1100.0.11

### Patch Changes

- 27425d7: Pin the integrity of git-hosted tarballs (codeload.github.com, gitlab.com, bitbucket.org) in the lockfile so that subsequent installs detect a tampered or substituted tarball and refuse to install it. Previously the lockfile only stored the tarball URL for git dependencies, so a compromised git host or a man-in-the-middle could serve arbitrary code on later installs without lockfile changes.

  A new `gitHosted: true` field is recorded on git-hosted tarball resolutions in the lockfile, letting every reader/writer route them by a single typed check instead of pattern-matching the tarball URL in each call site. Lockfiles written by older pnpm versions are enriched on load (URL fallback) so the field can be relied on uniformly across the codebase.

- Updated dependencies [27425d7]
- Updated dependencies [707a879]
  - @pnpm/lockfile.types@1100.0.4
  - @pnpm/lockfile.utils@1100.0.5
  - @pnpm/config.reader@1101.2.1
  - @pnpm/installing.context@1100.0.6
  - @pnpm/installing.client@1100.0.10
  - @pnpm/store.controller-types@1100.0.5
  - @pnpm/store.connection-manager@1100.0.11
  - @pnpm/store.cafs@1100.1.2

## 1100.0.10

### Patch Changes

- Updated dependencies [8fdd9a9]
- Updated dependencies [5f34a8d]
- Updated dependencies [c969392]
- Updated dependencies [817b1b4]
- Updated dependencies [c969392]
- Updated dependencies [2de318b]
  - @pnpm/config.reader@1101.2.0
  - @pnpm/store.connection-manager@1100.0.10
  - @pnpm/installing.client@1100.0.9

## 1100.0.9

### Patch Changes

- Updated dependencies [42a8f29]
  - @pnpm/config.reader@1101.1.4
  - @pnpm/store.connection-manager@1100.0.9
  - @pnpm/installing.client@1100.0.8

## 1100.0.8

### Patch Changes

- Updated dependencies [184ce26]
- Updated dependencies [5a901e7]
- Updated dependencies [6b891a5]
  - @pnpm/resolving.parse-wanted-dependency@1100.0.1
  - @pnpm/config.normalize-registries@1100.0.2
  - @pnpm/store.connection-manager@1100.0.8
  - @pnpm/store.controller-types@1100.0.4
  - @pnpm/installing.context@1100.0.5
  - @pnpm/installing.client@1100.0.7
  - @pnpm/fs.graceful-fs@1100.1.0
  - @pnpm/config.reader@1101.1.3
  - @pnpm/store.path@1100.0.1
  - @pnpm/cli.utils@1101.0.2
  - @pnpm/deps.path@1100.0.2
  - @pnpm/lockfile.utils@1100.0.4
  - @pnpm/lockfile.types@1100.0.3
  - @pnpm/store.cafs@1100.1.1
  - @pnpm/global.packages@1100.0.2

## 1100.0.7

### Patch Changes

- @pnpm/cli.utils@1101.0.1
- @pnpm/installing.context@1100.0.4
- @pnpm/store.connection-manager@1100.0.7

## 1100.0.6

### Patch Changes

- Updated dependencies [0fbcf74]
  - @pnpm/config.reader@1101.1.2
  - @pnpm/store.connection-manager@1100.0.6
  - @pnpm/installing.client@1100.0.6

## 1100.0.5

### Patch Changes

- @pnpm/installing.client@1100.0.5
- @pnpm/store.connection-manager@1100.0.5

## 1100.0.4

### Patch Changes

- Updated dependencies [421317c]
  - @pnpm/store.cafs@1100.1.0
  - @pnpm/installing.client@1100.0.4
  - @pnpm/store.controller-types@1100.0.3
  - @pnpm/store.connection-manager@1100.0.4
  - @pnpm/lockfile.utils@1100.0.3
  - @pnpm/installing.context@1100.0.3
  - @pnpm/config.reader@1101.1.1

## 1100.0.3

### Patch Changes

- Updated dependencies [7d25bc1]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0
  - @pnpm/store.connection-manager@1100.0.3
  - @pnpm/installing.client@1100.0.3
  - @pnpm/installing.context@1100.0.2
  - @pnpm/lockfile.types@1100.0.2
  - @pnpm/lockfile.utils@1100.0.2
  - @pnpm/store.controller-types@1100.0.2
  - @pnpm/store.cafs@1100.0.2

## 1100.0.2

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/cli.utils@1101.0.0
  - @pnpm/config.reader@1101.0.0
  - @pnpm/store.connection-manager@1100.0.2
  - @pnpm/installing.client@1100.0.2

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
