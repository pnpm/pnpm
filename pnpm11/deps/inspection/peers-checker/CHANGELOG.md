# @pnpm/deps.inspection.peers-checker

## 1100.0.16

### Patch Changes

- Updated dependencies [852d537]
  - @pnpm/error@1100.0.1
  - @pnpm/lockfile.fs@1100.1.7
  - @pnpm/config.parse-overrides@1100.0.2
  - @pnpm/lockfile.walker@1100.0.12

## 1100.0.15

### Patch Changes

- 3188ae7: Fixed `pnpm peers check` to accept loose peer dependency ranges such as `>=3.16.0 || >=4.0.0-` when the installed peer version satisfies the range [#12149](https://github.com/pnpm/pnpm/issues/12149).
- Updated dependencies [61969fb]
  - @pnpm/lockfile.fs@1100.1.6

## 1100.0.14

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [681b593]
- Updated dependencies [d50d691]
- Updated dependencies [a31faa7]
  - @pnpm/types@1101.3.2
  - @pnpm/lockfile.fs@1100.1.5
  - @pnpm/deps.path@1100.0.8
  - @pnpm/lockfile.walker@1100.0.11

## 1100.0.13

### Patch Changes

- Updated dependencies [bf1b731]
  - @pnpm/types@1101.3.1
  - @pnpm/deps.path@1100.0.7
  - @pnpm/lockfile.fs@1100.1.4
  - @pnpm/lockfile.walker@1100.0.10

## 1100.0.12

### Patch Changes

- Updated dependencies [a017bf3]
  - @pnpm/types@1101.3.0
  - @pnpm/deps.path@1100.0.6
  - @pnpm/lockfile.fs@1100.1.3
  - @pnpm/lockfile.walker@1100.0.9

## 1100.0.11

### Patch Changes

- Updated dependencies [35d2355]
  - @pnpm/types@1101.2.0
  - @pnpm/lockfile.fs@1100.1.2
  - @pnpm/deps.path@1100.0.5
  - @pnpm/lockfile.walker@1100.0.8

## 1100.0.10

### Patch Changes

- Updated dependencies [9cb48bb]
- Updated dependencies [64afc92]
  - @pnpm/lockfile.fs@1100.1.1
  - @pnpm/types@1101.1.1
  - @pnpm/deps.path@1100.0.4
  - @pnpm/lockfile.walker@1100.0.7

## 1100.0.9

### Patch Changes

- Updated dependencies [6e93f35]
- Updated dependencies [2a9bd89]
  - @pnpm/lockfile.fs@1100.1.0
  - @pnpm/lockfile.walker@1100.0.6

## 1100.0.8

### Patch Changes

- Updated dependencies [180aee9]
  - @pnpm/lockfile.fs@1100.0.8

## 1100.0.7

### Patch Changes

- Updated dependencies [b61e268]
  - @pnpm/types@1101.1.0
  - @pnpm/deps.path@1100.0.3
  - @pnpm/lockfile.fs@1100.0.7
  - @pnpm/lockfile.walker@1100.0.5

## 1100.0.6

### Patch Changes

- @pnpm/lockfile.fs@1100.0.6

## 1100.0.5

### Patch Changes

- Updated dependencies [27425d7]
  - @pnpm/lockfile.fs@1100.0.5
  - @pnpm/lockfile.walker@1100.0.4

## 1100.0.4

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/config.parse-overrides@1100.0.1
  - @pnpm/config.matcher@1100.0.1
  - @pnpm/deps.path@1100.0.2
  - @pnpm/lockfile.fs@1100.0.4
  - @pnpm/lockfile.walker@1100.0.3

## 1100.0.3

### Patch Changes

- @pnpm/lockfile.fs@1100.0.3

## 1100.0.2

### Patch Changes

- @pnpm/lockfile.fs@1100.0.2
- @pnpm/lockfile.walker@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/deps.path@1100.0.1
  - @pnpm/lockfile.fs@1100.0.1
  - @pnpm/lockfile.walker@1100.0.1

## 1000.1.0

### Minor Changes

- 263a8bc: Added `pnpm peers check` command that checks for unmet and missing peer dependency issues by reading the lockfile [#7087](https://github.com/pnpm/pnpm/issues/7087).

### Patch Changes

- Updated dependencies [5f73b0f]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [491a84f]
- Updated dependencies [d458ab3]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [1e6de25]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
  - @pnpm/deps.path@1002.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/lockfile.fs@1002.0.0
  - @pnpm/config.parse-overrides@1002.0.0
  - @pnpm/lockfile.walker@1002.0.0
  - @pnpm/config.matcher@1001.0.0
  - @pnpm/error@1001.0.0
