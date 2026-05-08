# @pnpm/building.commands

## 1100.0.14

### Patch Changes

- Updated dependencies [e9e876c]
- Updated dependencies [15e9e35]
  - @pnpm/config.reader@1101.2.2
  - @pnpm/installing.commands@1100.1.12
  - @pnpm/building.after-install@1101.0.9
  - @pnpm/store.connection-manager@1100.0.13

## 1100.0.13

### Patch Changes

- @pnpm/building.after-install@1101.0.8
- @pnpm/installing.commands@1100.1.11
- @pnpm/store.connection-manager@1100.0.12

## 1100.0.12

### Patch Changes

- Updated dependencies [12313f1]
- Updated dependencies [27425d7]
- Updated dependencies [707a879]
  - @pnpm/installing.modules-yaml@1100.0.3
  - @pnpm/building.after-install@1101.0.7
  - @pnpm/config.reader@1101.2.1
  - @pnpm/installing.commands@1100.1.10
  - @pnpm/store.connection-manager@1100.0.11
  - @pnpm/config.writer@1100.0.6

## 1100.0.11

### Patch Changes

- Updated dependencies [8fdd9a9]
- Updated dependencies [5f34a8d]
- Updated dependencies [c969392]
- Updated dependencies [817b1b4]
- Updated dependencies [c969392]
- Updated dependencies [2de318b]
  - @pnpm/config.reader@1101.2.0
  - @pnpm/building.after-install@1101.0.6
  - @pnpm/installing.commands@1100.1.9
  - @pnpm/store.connection-manager@1100.0.10
  - @pnpm/config.writer@1100.0.5

## 1100.0.10

### Patch Changes

- f6bc1db: `pnpm dlx` (and `pnpx`/`pnx`/`pnpm create`) now runs the same interactive `approve-builds` prompt as `pnpm add -g` when the package being launched depends on transitive packages with install scripts. Previously, the v11 `strictDepBuilds` default made dlx fail with `ERR_PNPM_IGNORED_BUILDS` and required users to re-run with `--allow-build=<pkg>` for every offending dependency. dlx also now removes the partially-populated cache directory when the install fails, so a subsequent run starts clean instead of reusing a broken install whose builds were silently skipped [#11444](https://github.com/pnpm/pnpm/issues/11444).
- Updated dependencies [f6bc1db]
  - @pnpm/installing.commands@1100.1.8

## 1100.0.9

### Patch Changes

- Updated dependencies [42a8f29]
  - @pnpm/config.reader@1101.1.4
  - @pnpm/building.after-install@1101.0.5
  - @pnpm/installing.commands@1100.1.7
  - @pnpm/store.connection-manager@1100.0.9

## 1100.0.8

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/cli.common-cli-options-help@1100.0.1
  - @pnpm/store.connection-manager@1100.0.8
  - @pnpm/installing.modules-yaml@1100.0.2
  - @pnpm/installing.commands@1100.1.6
  - @pnpm/config.reader@1101.1.3
  - @pnpm/config.writer@1100.0.4
  - @pnpm/cli.command@1100.0.1
  - @pnpm/cli.utils@1101.0.2
  - @pnpm/deps.path@1100.0.2
  - @pnpm/building.after-install@1101.0.4

## 1100.0.7

### Patch Changes

- @pnpm/cli.utils@1101.0.1
- @pnpm/installing.commands@1100.1.5
- @pnpm/building.after-install@1101.0.3
- @pnpm/store.connection-manager@1100.0.7

## 1100.0.6

### Patch Changes

- Updated dependencies [0fbcf74]
  - @pnpm/config.reader@1101.1.2
  - @pnpm/building.after-install@1101.0.2
  - @pnpm/installing.commands@1100.1.4
  - @pnpm/store.connection-manager@1100.0.6
  - @pnpm/config.writer@1100.0.3

## 1100.0.5

### Patch Changes

- @pnpm/installing.commands@1100.1.3
- @pnpm/store.connection-manager@1100.0.5
- @pnpm/building.after-install@1101.0.1

## 1100.0.4

### Patch Changes

- @pnpm/building.after-install@1101.0.0
- @pnpm/installing.commands@1100.1.2
- @pnpm/store.connection-manager@1100.0.4
- @pnpm/config.reader@1101.1.1

## 1100.0.3

### Patch Changes

- 7d9aae9: Fix `ERR_PNPM_OUTDATED_LOCKFILE` when approving builds during a global install. The `approve-builds` flow called by `pnpm add -g` passed the global packages directory to the subsequent install as `workspaceDir`, which caused sibling install directories (such as those left behind by `pnpm self-update`) to be picked up as workspace projects and fail the frozen-lockfile check.
- Updated dependencies [7d25bc1]
- Updated dependencies [72c1e05]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0
  - @pnpm/building.after-install@1100.0.3
  - @pnpm/store.connection-manager@1100.0.3
  - @pnpm/installing.commands@1100.1.1
  - @pnpm/config.writer@1100.0.2

## 1100.0.2

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/cli.utils@1101.0.0
  - @pnpm/config.reader@1101.0.0
  - @pnpm/installing.commands@1100.1.0
  - @pnpm/building.after-install@1100.0.2
  - @pnpm/store.connection-manager@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/building.after-install@1100.0.1
  - @pnpm/cli.utils@1100.0.1
  - @pnpm/config.reader@1100.0.1
  - @pnpm/config.writer@1100.0.1
  - @pnpm/deps.path@1100.0.1
  - @pnpm/installing.commands@1100.0.1
  - @pnpm/installing.modules-yaml@1100.0.1
  - @pnpm/workspace.projects-sorter@1100.0.1
  - @pnpm/store.connection-manager@1100.0.1

## 1000.0.0

### Major Changes

- 2fccb03: Initial release
- 05fb1ae: `ignoreBuilds` is now a set of DepPath.
- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.
- cb367b9: Remove deprecated build dependency settings: `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies`.

### Minor Changes

- 2e8816e: Added `--all` flag to `pnpm approve-builds` that approves all pending builds without interactive prompts [#10136](https://github.com/pnpm/pnpm/issues/10136).
- 996284f: Allow `pnpm approve-builds` to receive positional arguments for approving or denying packages without the interactive prompt. Prefix a package name with `!` to deny it. Only mentioned packages are affected; the rest are left untouched.

  During install, packages with ignored builds that are not yet listed in `allowBuilds` are automatically added with a placeholder value. This makes them visible in `pnpm-workspace.yaml` so users can manually change them to `true` or `false` without running `pnpm approve-builds`.

- b7f0f21: Use SQLite for storing package index in the content-addressable store. Instead of individual `.mpk` files under `$STORE/index/`, package metadata is now stored in a single SQLite database at `$STORE/index.db`. This reduces filesystem syscall overhead, improves space efficiency for small metadata entries, and enables concurrent access via SQLite's WAL mode. Packages missing from the new index are re-fetched on demand [#10826](https://github.com/pnpm/pnpm/issues/10826).

### Patch Changes

- 9fc552d: In GVS mode, `pnpm approve-builds` now runs a full install instead of rebuild. This ensures that GVS hash directories and symlinks are updated correctly after changing `allowBuilds`, preventing build artifact contamination of engine-agnostic directories [#11042](https://github.com/pnpm/pnpm/issues/11042).
- 4362c06: `pnpm install` should build any dependencies that were added to `onlyBuiltDependencies` and were not built yet [#10256](https://github.com/pnpm/pnpm/pull/10256).
- Updated dependencies [e1ea779]
- Updated dependencies [7730a7f]
- Updated dependencies [5f73b0f]
- Updated dependencies [996284f]
- Updated dependencies [7721d2e]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [4c6c26a]
- Updated dependencies [3c72b6b]
- Updated dependencies [9f5c0e3]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [90bd3c3]
- Updated dependencies [3cfffaa]
- Updated dependencies [1cc61e8]
- Updated dependencies [606f53e]
- Updated dependencies [c7203b9]
- Updated dependencies [bb17724]
- Updated dependencies [2fccb03]
- Updated dependencies [05fb1ae]
- Updated dependencies [da2429d]
- Updated dependencies [0b5ccc9]
- Updated dependencies [1cc61e8]
- Updated dependencies [491a84f]
- Updated dependencies [f0ae1b9]
- Updated dependencies [9fc552d]
- Updated dependencies [312226c]
- Updated dependencies [121f64a]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [075aa99]
- Updated dependencies [fd511e4]
- Updated dependencies [ae43ac7]
- Updated dependencies [ccec8e7]
- Updated dependencies [fd511e4]
- Updated dependencies [fa5a5c6]
- Updated dependencies [4158906]
- Updated dependencies [ac944ef]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [96704a1]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [6f806be]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [ace7903]
- Updated dependencies [38b8e35]
- Updated dependencies [1e6de25]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [ed1a7fe]
- Updated dependencies [15549a9]
- Updated dependencies [b51bb42]
- Updated dependencies [cc7c0d2]
- Updated dependencies [5bf7768]
- Updated dependencies [3cfffaa]
- Updated dependencies [ae43ac7]
- Updated dependencies [a5fdbf9]
- Updated dependencies [7354e6b]
- Updated dependencies [9d3f00b]
- Updated dependencies [efb48dc]
- Updated dependencies [9587dac]
- Updated dependencies [09a999a]
- Updated dependencies [559f903]
- Updated dependencies [3574905]
  - @pnpm/cli.common-cli-options-help@1001.0.0
  - @pnpm/config.reader@1005.0.0
  - @pnpm/deps.path@1002.0.0
  - @pnpm/building.after-install@1000.0.0
  - @pnpm/installing.commands@1005.0.0
  - @pnpm/config.writer@1001.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/cli.utils@1002.0.0
  - @pnpm/installing.modules-yaml@1001.0.0
  - @pnpm/store.connection-manager@1003.0.0
  - @pnpm/prepare-temp-dir@1001.0.0
  - @pnpm/workspace.projects-sorter@1001.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/cli.command@1001.0.0
