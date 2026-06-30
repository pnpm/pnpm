# @pnpm/global.packages

## 1100.0.9

### Patch Changes

- @pnpm/pkg-manifest.reader@1100.0.9
- @pnpm/crypto.hash@1100.0.1

## 1100.0.8

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [681b593]
  - @pnpm/types@1101.3.2
  - @pnpm/bins.resolver@1100.0.8
  - @pnpm/pkg-manifest.reader@1100.0.8
  - @pnpm/crypto.hash@1100.0.1

## 1100.0.7

### Patch Changes

- Updated dependencies [230df57]
- Updated dependencies [bf1b731]
  - @pnpm/bins.resolver@1100.0.7
  - @pnpm/types@1101.3.1
  - @pnpm/pkg-manifest.reader@1100.0.7
  - @pnpm/crypto.hash@1100.0.1

## 1100.0.6

### Patch Changes

- Updated dependencies [a017bf3]
  - @pnpm/types@1101.3.0
  - @pnpm/bins.resolver@1100.0.6
  - @pnpm/pkg-manifest.reader@1100.0.6
  - @pnpm/crypto.hash@1100.0.1

## 1100.0.5

### Patch Changes

- Updated dependencies [35d2355]
  - @pnpm/types@1101.2.0
  - @pnpm/bins.resolver@1100.0.5
  - @pnpm/pkg-manifest.reader@1100.0.5
  - @pnpm/crypto.hash@1100.0.1

## 1100.0.4

### Patch Changes

- Updated dependencies [64afc92]
  - @pnpm/types@1101.1.1
  - @pnpm/bins.resolver@1100.0.4
  - @pnpm/pkg-manifest.reader@1100.0.4
  - @pnpm/crypto.hash@1100.0.1

## 1100.0.3

### Patch Changes

- Updated dependencies [b61e268]
  - @pnpm/types@1101.1.0
  - @pnpm/bins.resolver@1100.0.3
  - @pnpm/pkg-manifest.reader@1100.0.3
  - @pnpm/crypto.hash@1100.0.1

## 1100.0.2

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/pkg-manifest.reader@1100.0.2
  - @pnpm/bins.resolver@1100.0.2
  - @pnpm/crypto.hash@1100.0.1

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/bins.resolver@1100.0.1
  - @pnpm/pkg-manifest.reader@1100.0.1
  - @pnpm/crypto.hash@1100.0.0

## 1000.0.0

### Minor Changes

- fd511e4: Isolated global packages. Each globally installed package (or group of packages installed together) now gets its own isolated installation directory with its own `package.json`, `node_modules/`, and lockfile. This prevents global packages from interfering with each other through peer dependency conflicts, hoisting changes, or version resolution shifts.

  Key changes:

  - `pnpm add -g <pkg>` creates an isolated installation in `{pnpmHomeDir}/global/v11/{hash}/`
  - `pnpm remove -g <pkg>` removes the entire installation group containing the package
  - `pnpm update -g [pkg]` re-installs packages in new isolated directories
  - `pnpm list -g` scans isolated directories to show all installed global packages
  - `pnpm install -g` (no args) is no longer supported; use `pnpm add -g <pkg>` instead

### Patch Changes

- Updated dependencies [449dacf]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [491a84f]
- Updated dependencies [13855ac]
- Updated dependencies [d7b8be4]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
  - @pnpm/bins.resolver@1001.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/pkg-manifest.reader@1001.0.0
  - @pnpm/crypto.hash@1001.0.0
