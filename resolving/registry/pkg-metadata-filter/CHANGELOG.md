# @pnpm/registry.pkg-metadata-filter

## 1100.0.9

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.
  - @pnpm/resolving.registry.types@1100.1.3

## 1100.0.8

### Patch Changes

- @pnpm/resolving.registry.types@1100.1.2

## 1100.0.7

### Patch Changes

- @pnpm/resolving.registry.types@1100.1.1

## 1100.0.6

### Patch Changes

- Updated dependencies [1e9ab29]
  - @pnpm/resolving.registry.types@1100.1.0

## 1100.0.5

### Patch Changes

- @pnpm/resolving.registry.types@1100.0.5

## 1100.0.4

### Patch Changes

- @pnpm/resolving.registry.types@1100.0.4

## 1100.0.3

### Patch Changes

- @pnpm/resolving.registry.types@1100.0.3

## 1100.0.2

### Patch Changes

- 184ce26: Fix the package name in README.md.
- Updated dependencies [184ce26]
  - @pnpm/resolving.registry.types@1100.0.2

## 1100.0.1

### Patch Changes

- @pnpm/resolving.registry.types@1100.0.1

## 1000.1.2

### Patch Changes

- Updated dependencies [d3d6938]
- Updated dependencies [10bc391]
  - @pnpm/resolving.registry.types@1000.1.0

## 1000.1.1

### Patch Changes

- 0152a51: When the `latest` version doesn't satisfy the maturity requirement configured by `minimumReleaseAge`, pick the highest version that is mature enough, even if it has a different major version [#10100](https://github.com/pnpm/pnpm/issues/10100).

## 1000.1.0

### Minor Changes

- 7c1382f: Allow excluding certain trusted versions from the date check.

### Patch Changes

- @pnpm/registry.types@1000.0.1

## 1000.0.0

### Major Changes

- 4a2d871: Initial release.

### Patch Changes

- Updated dependencies [4a2d871]
  - @pnpm/registry.types@1000.0.0
