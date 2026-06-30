# @pnpm/resolve-workspace-range

## 1100.0.2

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

## 1100.0.1

### Patch Changes

- 184ce26: Fix the package name in README.md.

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- 0625e20: Support bare `workspace:` protocol without version specifier. It is now treated as `workspace:*` and resolves to the concrete version during publish [#10436](https://github.com/pnpm/pnpm/pull/10436).

## 6.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

## 5.0.1

### Patch Changes

- c0760128d: bump semver to 7.4.0

## 5.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

## 4.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

## 3.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

## 2.1.0

### Minor Changes

- 85fb21a83: Add support for workspace:^ and workspace:~ aliases

## 2.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

## 1.0.2
