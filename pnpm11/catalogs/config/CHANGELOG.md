# @pnpm/catalogs.config

## 1100.0.2

### Patch Changes

- Updated dependencies [852d537]
  - @pnpm/error@1100.0.1

## 1100.0.1

### Patch Changes

- eba03e0: Fix `pnpm install` reporting "Already up to date" after a catalog entry in `pnpm-workspace.yaml` was reverted to a previous version. After an update modified a catalog, the workspace state cache stored the pre-update catalog versions, so reverting the entry back to its original version was not detected as an outdated state [#12418](https://github.com/pnpm/pnpm/issues/12418).

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Patch Changes

- Updated dependencies [491a84f]
- Updated dependencies [7d2fd48]
- Updated dependencies [831f574]
  - @pnpm/error@1001.0.0

## 1000.0.5

### Patch Changes

- @pnpm/error@1000.0.5

## 1000.0.4

### Patch Changes

- @pnpm/error@1000.0.4

## 1000.0.3

### Patch Changes

- @pnpm/error@1000.0.3

## 1000.0.2

### Patch Changes

- @pnpm/error@1000.0.2

## 1000.0.1

### Patch Changes

- @pnpm/error@1000.0.1

## 0.1.2

### Patch Changes

- @pnpm/error@6.0.3

## 0.1.1

### Patch Changes

- @pnpm/error@6.0.2

## 0.1.0

Initial release
