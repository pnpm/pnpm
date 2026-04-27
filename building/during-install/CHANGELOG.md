# @pnpm/building.during-install

## 1101.0.0

### Patch Changes

- Updated dependencies [421317c]
  - @pnpm/worker@1100.1.0
  - @pnpm/store.controller-types@1100.0.3
  - @pnpm/exec.lifecycle@1100.0.4
  - @pnpm/deps.graph-hasher@1100.1.1
  - @pnpm/config.reader@1101.1.1
  - @pnpm/fs.hard-link-dir@1100.0.0
  - @pnpm/patching.apply-patch@1100.0.0

## 1100.0.3

### Patch Changes

- Updated dependencies [7d25bc1]
- Updated dependencies [72c1e05]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0
  - @pnpm/deps.graph-hasher@1100.1.0
  - @pnpm/exec.lifecycle@1100.0.3
  - @pnpm/store.controller-types@1100.0.2
  - @pnpm/worker@1100.0.2
  - @pnpm/fs.hard-link-dir@1100.0.0
  - @pnpm/patching.apply-patch@1100.0.0

## 1100.0.2

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/config.reader@1101.0.0
  - @pnpm/bins.linker@1100.0.2
  - @pnpm/exec.lifecycle@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/bins.linker@1100.0.1
  - @pnpm/config.reader@1100.0.1
  - @pnpm/core-loggers@1100.0.1
  - @pnpm/deps.graph-hasher@1100.0.1
  - @pnpm/deps.path@1100.0.1
  - @pnpm/exec.lifecycle@1100.0.1
  - @pnpm/pkg-manifest.reader@1100.0.1
  - @pnpm/store.controller-types@1100.0.1
  - @pnpm/worker@1100.0.1
  - @pnpm/fs.hard-link-dir@1100.0.0
  - @pnpm/patching.apply-patch@1100.0.0

## 1000.0.0

### Major Changes

- 2fccb03: Initial release
- 05fb1ae: `ignoreBuilds` is now a set of DepPath.
- efb48dc: Replaced fetchingBundledManifest with fetching.
- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.
- 56a59df: Store the bundled manifest (name, version, bin, engines, scripts, etc.) directly in the package index file, eliminating the need to read `package.json` from the content-addressable store during resolution and installation. This reduces I/O and speeds up repeat installs [#10473](https://github.com/pnpm/pnpm/pull/10473).
- 7b1c189: Removed the deprecated `allowNonAppliedPatches` completely in favor of `allowUnusedPatches`.
  Remove `ignorePatchFailures` so all patch application failures should throw an error.

### Patch Changes

- 9b801c8: Fixed `strictDepBuilds` and `allowBuilds` checks being bypassed when a package's build side-effects are cached in the store. Packages with cached builds were skipped by `buildModules` (`isBuilt: true`) and never reached the `allowBuild` check. Now checks `allowBuild` for all packages with `requiresBuild` regardless of `isBuilt` state. Also detects packages whose build approval was revoked between installs.
- 56a59df: Defer patch errors until all patches in a group are applied, so that one failed patch does not prevent other patches from being attempted.
- 4362c06: `pnpm install` should build any dependencies that were added to `onlyBuiltDependencies` and were not built yet [#10256](https://github.com/pnpm/pnpm/pull/10256).
- Updated dependencies [7730a7f]
- Updated dependencies [5f73b0f]
- Updated dependencies [449dacf]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [e2e0a32]
- Updated dependencies [3c72b6b]
- Updated dependencies [9f5c0e3]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [90bd3c3]
- Updated dependencies [7cec347]
- Updated dependencies [2a50b89]
- Updated dependencies [1cc61e8]
- Updated dependencies [606f53e]
- Updated dependencies [c7203b9]
- Updated dependencies [bb17724]
- Updated dependencies [cd743ef]
- Updated dependencies [da2429d]
- Updated dependencies [1cc61e8]
- Updated dependencies [491a84f]
- Updated dependencies [62f760e]
- Updated dependencies [f0ae1b9]
- Updated dependencies [6e9cad3]
- Updated dependencies [50fbeca]
- Updated dependencies [cb228c9]
- Updated dependencies [0fd53e1]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [075aa99]
- Updated dependencies [c4045fc]
- Updated dependencies [ae43ac7]
- Updated dependencies [ccec8e7]
- Updated dependencies [ba065f6]
- Updated dependencies [3bf5e21]
- Updated dependencies [4158906]
- Updated dependencies [ac944ef]
- Updated dependencies [a0e3a21]
- Updated dependencies [ee9fe58]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
- Updated dependencies [56a59df]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [780af09]
- Updated dependencies [96704a1]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [f40177f]
- Updated dependencies [71de2b3]
- Updated dependencies [4893853]
- Updated dependencies [10bc391]
- Updated dependencies [b09722f]
- Updated dependencies [38b8e35]
- Updated dependencies [b7f0f21]
- Updated dependencies [1e6de25]
- Updated dependencies [831f574]
- Updated dependencies [366cabe]
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
- Updated dependencies [f871365]
  - @pnpm/config.reader@1005.0.0
  - @pnpm/deps.path@1002.0.0
  - @pnpm/deps.graph-hasher@1003.0.0
  - @pnpm/bins.linker@1001.0.0
  - @pnpm/store.controller-types@1005.0.0
  - @pnpm/worker@1001.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/fs.hard-link-dir@1001.0.0
  - @pnpm/pkg-manifest.reader@1001.0.0
  - @pnpm/core-loggers@1002.0.0
  - @pnpm/deps.graph-sequencer@1001.0.0
  - @pnpm/patching.apply-patch@1001.0.0
  - @pnpm/exec.lifecycle@1002.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/patching.types@1001.0.0
