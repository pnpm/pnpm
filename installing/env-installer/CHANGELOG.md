# @pnpm/config.deps-installer

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/config.pick-registry-for-package@1100.0.1
  - @pnpm/config.writer@1100.0.1
  - @pnpm/core-loggers@1100.0.1
  - @pnpm/deps.graph-hasher@1100.0.1
  - @pnpm/installing.deps-resolver@1100.0.1
  - @pnpm/lockfile.fs@1100.0.1
  - @pnpm/lockfile.pruner@1100.0.1
  - @pnpm/lockfile.types@1100.0.1
  - @pnpm/lockfile.utils@1100.0.1
  - @pnpm/network.auth-header@1100.0.1
  - @pnpm/network.fetch@1100.0.1
  - @pnpm/pkg-manifest.reader@1100.0.1
  - @pnpm/resolving.npm-resolver@1100.0.1
  - @pnpm/store.controller@1100.0.1
  - @pnpm/store.controller-types@1100.0.1
  - @pnpm/worker@1100.0.1

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- 821b36a: Config dependencies are now installed into the global virtual store (`{storeDir}/links/`) and symlinked into `node_modules/.pnpm-config/`. This allows config dependencies to be shared across projects that use the same store, avoiding redundant fetches and imports.
- a8f016c: Store config dependency and package manager integrity info in `pnpm-lock.yaml` instead of inlining it in `pnpm-workspace.yaml`. The workspace manifest now contains only clean version specifiers for `configDependencies`, while the resolved versions, integrity hashes, and tarball URLs are recorded in the lockfile as a separate YAML document. The env lockfile section also stores `packageManagerDependencies` resolved during version switching and self-update. Projects using the old inline-hash format are automatically migrated on install.
- cc1b8e3: Fixed installation of config dependencies from private registries.

  Added support for object type in `configDependencies` when the tarball URL returned from package metadata differs from the computed URL [#10431](https://github.com/pnpm/pnpm/pull/10431).

- d8be970: Throws `FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` when attempting to install configuration dependencies with `--frozen-lockfile` active and the env lockfile is missing or out-of-date. Previously, the operation would silently rewrite the workspace file or resolve in-memory.
- 4a36b9a: Refactor workspace domains: rename `project-finder` to `projects-reader`, merge `filter-packages-from-dir` into `filter-workspace-packages`, and rename it to `projects-filter`. Also, move and rename `config/deps-installer` to `installing/env-installer`.

### Patch Changes

- Updated dependencies [5f73b0f]
- Updated dependencies [7721d2e]
- Updated dependencies [ae8b816]
- Updated dependencies [f98a2db]
- Updated dependencies [facdd71]
- Updated dependencies [e2e0a32]
- Updated dependencies [c55c614]
- Updated dependencies [a297ebc]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [5a0ed1d]
- Updated dependencies [7cec347]
- Updated dependencies [606f53e]
- Updated dependencies [831f574]
- Updated dependencies [0e9c559]
- Updated dependencies [e46a652]
- Updated dependencies [cd743ef]
- Updated dependencies [19f36cf]
- Updated dependencies [491a84f]
- Updated dependencies [94571fb]
- Updated dependencies [fb8962f]
- Updated dependencies [54c4fc4]
- Updated dependencies [e73da5e]
- Updated dependencies [61cad0c]
- Updated dependencies [b1ad9c7]
- Updated dependencies [50fbeca]
- Updated dependencies [2fc9139]
- Updated dependencies [19f36cf]
- Updated dependencies [0dfa8b8]
- Updated dependencies [121f64a]
- Updated dependencies [9eddabb]
- Updated dependencies [075aa99]
- Updated dependencies [c4045fc]
- Updated dependencies [143ca78]
- Updated dependencies [ba065f6]
- Updated dependencies [3bf5e21]
- Updated dependencies [6f361aa]
- Updated dependencies [0625e20]
- Updated dependencies [938ea1f]
- Updated dependencies [83fe533]
- Updated dependencies [2cb0657]
- Updated dependencies [bb8baa7]
- Updated dependencies [ee9fe58]
- Updated dependencies [d458ab3]
- Updated dependencies [021f70d]
- Updated dependencies [7d2fd48]
- Updated dependencies [9eddabb]
- Updated dependencies [144ce0e]
- Updated dependencies [efb48dc]
- Updated dependencies [56a59df]
- Updated dependencies [780af09]
- Updated dependencies [50fbeca]
- Updated dependencies [bb8baa7]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [6c480a4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [cee1f58]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [4893853]
- Updated dependencies [10bc391]
- Updated dependencies [ba70035]
- Updated dependencies [3585d9a]
- Updated dependencies [38b8e35]
- Updated dependencies [394d88c]
- Updated dependencies [b7f0f21]
- Updated dependencies [1e6de25]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [2f98ec8]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [4f3ad23]
- Updated dependencies [09bb8db]
- Updated dependencies [9d3f00b]
- Updated dependencies [6557dc0]
- Updated dependencies [98a0410]
- Updated dependencies [efb48dc]
- Updated dependencies [6b3d87a]
  - @pnpm/installing.deps-resolver@1009.0.0
  - @pnpm/deps.graph-hasher@1003.0.0
  - @pnpm/config.writer@1001.0.0
  - @pnpm/store.controller-types@1005.0.0
  - @pnpm/resolving.npm-resolver@1005.0.0
  - @pnpm/worker@1001.0.0
  - @pnpm/store.controller@1005.0.0
  - @pnpm/constants@1002.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/lockfile.fs@1002.0.0
  - @pnpm/lockfile.types@1003.0.0
  - @pnpm/lockfile.utils@1004.0.0
  - @pnpm/config.pick-registry-for-package@1001.0.0
  - @pnpm/resolving.parse-wanted-dependency@1002.0.0
  - @pnpm/pkg-manifest.reader@1001.0.0
  - @pnpm/core-loggers@1002.0.0
  - @pnpm/fs.read-modules-dir@1001.0.0
  - @pnpm/network.auth-header@1001.0.0
  - @pnpm/lockfile.pruner@1002.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/network.fetch@1001.0.0

## 1000.0.19

### Patch Changes

- Updated dependencies [6c3dcb8]
  - @pnpm/npm-resolver@1004.4.1
  - @pnpm/package-store@1004.0.0

## 1000.0.18

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0
  - @pnpm/npm-resolver@1004.4.0
  - @pnpm/package-store@1004.0.0
  - @pnpm/config.config-writer@1000.0.14
  - @pnpm/pick-registry-for-package@1000.0.11
  - @pnpm/fetch@1000.2.6
  - @pnpm/core-loggers@1001.0.4
  - @pnpm/read-package-json@1000.1.2

## 1000.0.17

### Patch Changes

- @pnpm/package-store@1003.0.0

## 1000.0.16

### Patch Changes

- Updated dependencies [fb4da0c]
  - @pnpm/npm-resolver@1004.3.0
  - @pnpm/package-store@1002.0.12
  - @pnpm/config.config-writer@1000.0.13

## 1000.0.15

### Patch Changes

- Updated dependencies [baf8bf6]
- Updated dependencies [702ddb9]
  - @pnpm/npm-resolver@1004.2.3
  - @pnpm/package-store@1002.0.11

## 1000.0.14

### Patch Changes

- Updated dependencies [121b44e]
- Updated dependencies [02f8b69]
  - @pnpm/npm-resolver@1004.2.2
  - @pnpm/package-store@1002.0.11

## 1000.0.13

### Patch Changes

- @pnpm/error@1000.0.5
- @pnpm/npm-resolver@1004.2.1
- @pnpm/network.auth-header@1000.0.6
- @pnpm/read-package-json@1000.1.1
- @pnpm/config.config-writer@1000.0.12
- @pnpm/package-store@1002.0.11

## 1000.0.12

### Patch Changes

- Updated dependencies [e792927]
- Updated dependencies [38e2599]
- Updated dependencies [e792927]
  - @pnpm/read-package-json@1000.1.0
  - @pnpm/npm-resolver@1004.2.0
  - @pnpm/types@1000.8.0
  - @pnpm/config.config-writer@1000.0.11
  - @pnpm/pick-registry-for-package@1000.0.10
  - @pnpm/fetch@1000.2.5
  - @pnpm/core-loggers@1001.0.3
  - @pnpm/package-store@1002.0.10

## 1000.0.11

### Patch Changes

- Updated dependencies [87d3aa8]
  - @pnpm/fetch@1000.2.4
  - @pnpm/config.config-writer@1000.0.10
  - @pnpm/npm-resolver@1004.1.3
  - @pnpm/package-store@1002.0.9

## 1000.0.10

### Patch Changes

- Updated dependencies [adb097c]
  - @pnpm/read-package-json@1000.0.11
  - @pnpm/error@1000.0.4
  - @pnpm/npm-resolver@1004.1.3
  - @pnpm/config.config-writer@1000.0.9
  - @pnpm/package-store@1002.0.9
  - @pnpm/network.auth-header@1000.0.5

## 1000.0.9

### Patch Changes

- Updated dependencies [1a07b8f]
  - @pnpm/types@1000.7.0
  - @pnpm/config.config-writer@1000.0.8
  - @pnpm/pick-registry-for-package@1000.0.9
  - @pnpm/fetch@1000.2.3
  - @pnpm/core-loggers@1001.0.2
  - @pnpm/read-package-json@1000.0.10
  - @pnpm/npm-resolver@1004.1.2
  - @pnpm/package-store@1002.0.8
  - @pnpm/error@1000.0.3
  - @pnpm/network.auth-header@1000.0.4

## 1000.0.8

### Patch Changes

- @pnpm/config.config-writer@1000.0.7
- @pnpm/npm-resolver@1004.1.1
- @pnpm/package-store@1002.0.7

## 1000.0.7

### Patch Changes

- @pnpm/package-store@1002.0.6

## 1000.0.6

### Patch Changes

- Updated dependencies [2721291]
  - @pnpm/npm-resolver@1004.1.0
  - @pnpm/package-store@1002.0.5
  - @pnpm/config.config-writer@1000.0.6

## 1000.0.5

### Patch Changes

- @pnpm/package-store@1002.0.4

## 1000.0.4

### Patch Changes

- 09cf46f: Update `@pnpm/logger` in peer dependencies.
- Updated dependencies [51bd373]
- Updated dependencies [09cf46f]
- Updated dependencies [5ec7255]
  - @pnpm/network.auth-header@1000.0.3
  - @pnpm/npm-resolver@1004.0.1
  - @pnpm/core-loggers@1001.0.1
  - @pnpm/package-store@1002.0.3
  - @pnpm/fetch@1000.2.2
  - @pnpm/types@1000.6.0
  - @pnpm/config.config-writer@1000.0.5
  - @pnpm/pick-registry-for-package@1000.0.8
  - @pnpm/read-package-json@1000.0.9

## 1000.0.3

### Patch Changes

- @pnpm/config.config-writer@1000.0.4
- @pnpm/package-store@1002.0.2

## 1000.0.2

### Patch Changes

- Updated dependencies [8a9f3a4]
- Updated dependencies [5b73df1]
- Updated dependencies [9c3dd03]
- Updated dependencies [5b73df1]
  - @pnpm/parse-wanted-dependency@1001.0.0
  - @pnpm/npm-resolver@1004.0.0
  - @pnpm/core-loggers@1001.0.0
  - @pnpm/logger@1001.0.0
  - @pnpm/types@1000.5.0
  - @pnpm/package-store@1002.0.2
  - @pnpm/fetch@1000.2.1
  - @pnpm/config.config-writer@1000.0.3
  - @pnpm/pick-registry-for-package@1000.0.7
  - @pnpm/read-package-json@1000.0.8

## 1000.0.1

### Patch Changes

- Updated dependencies [81f441c]
- Updated dependencies [17b7e9f]
  - @pnpm/npm-resolver@1003.0.0
  - @pnpm/config.config-writer@1000.0.2
  - @pnpm/package-store@1002.0.1

## 1000.0.0

### Major Changes

- 1413c25: Initial release.

### Minor Changes

- 750ae7d: Now you can use the `pnpm add` command with the `--config` flag to install new configurational dependencies [#9377](https://github.com/pnpm/pnpm/pull/9377).

### Patch Changes

- Updated dependencies [750ae7d]
- Updated dependencies [72cff38]
- Updated dependencies [750ae7d]
- Updated dependencies [750ae7d]
  - @pnpm/types@1000.4.0
  - @pnpm/npm-resolver@1002.0.0
  - @pnpm/package-store@1002.0.0
  - @pnpm/core-loggers@1000.2.0
  - @pnpm/fetch@1000.2.0
  - @pnpm/config.config-writer@1000.0.1
  - @pnpm/pick-registry-for-package@1000.0.6
  - @pnpm/read-package-json@1000.0.7
