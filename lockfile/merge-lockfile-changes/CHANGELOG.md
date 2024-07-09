# @pnpm/merge-lockfile-changes

## 6.0.4

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0
  - @pnpm/lockfile-types@7.1.2

## 6.0.3

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/lockfile-types@7.1.1

## 6.0.2

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/lockfile-types@7.1.0

## 6.0.1

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-types@7.0.0

## 6.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- d381a60: Support for lockfile v5 is dropped. Use pnpm v8 to convert lockfile v5 to lockfile v6 [#7470](https://github.com/pnpm/pnpm/pull/7470).

### Minor Changes

- 086b69c: The checksum of the `.pnpmfile.cjs` is saved into the lockfile. If the pnpmfile gets modified, the lockfile is reanalyzed to apply the changes [#7662](https://github.com/pnpm/pnpm/pull/7662).
- 730929e: Add a field named `ignoredOptionalDependencies`. This is an array of strings. If an optional dependency has its name included in this array, it will be skipped.

### Patch Changes

- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [27a96a8]
- Updated dependencies [730929e]
  - @pnpm/lockfile-types@6.0.0

## 5.0.7

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/lockfile-types@5.1.5

## 5.0.6

### Patch Changes

- Updated dependencies
  - @pnpm/lockfile-types@5.1.4

## 5.0.5

### Patch Changes

- @pnpm/lockfile-types@5.1.3

## 5.0.4

### Patch Changes

- @pnpm/lockfile-types@5.1.2

## 5.0.3

### Patch Changes

- @pnpm/lockfile-types@5.1.1

## 5.0.2

### Patch Changes

- Updated dependencies [9c4ae87bd]
  - @pnpm/lockfile-types@5.1.0

## 5.0.1

### Patch Changes

- c0760128d: bump semver to 7.4.0

## 5.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [c92936158]
- Updated dependencies [eceaa8b8b]
  - @pnpm/lockfile-types@5.0.0

## 4.0.3

### Patch Changes

- @pnpm/lockfile-types@4.3.6

## 4.0.2

### Patch Changes

- @pnpm/lockfile-types@4.3.5

## 4.0.1

### Patch Changes

- @pnpm/lockfile-types@4.3.4

## 4.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

## 3.0.11

### Patch Changes

- @pnpm/lockfile-types@4.3.3

## 3.0.10

### Patch Changes

- @pnpm/lockfile-types@4.3.2

## 3.0.9

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270

## 3.0.8

### Patch Changes

- @pnpm/lockfile-types@4.3.1

## 3.0.7

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-types@4.3.0

## 3.0.6

### Patch Changes

- 5f643f23b: Update ramda to v0.28.

## 3.0.5

### Patch Changes

- Updated dependencies [d01c32355]
- Updated dependencies [8e5b77ef6]
  - @pnpm/lockfile-types@4.2.0

## 3.0.4

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/lockfile-types@4.1.0

## 3.0.3

### Patch Changes

- @pnpm/lockfile-types@4.0.3

## 3.0.2

### Patch Changes

- @pnpm/lockfile-types@4.0.2

## 3.0.1

### Patch Changes

- @pnpm/lockfile-types@4.0.1

## 3.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/lockfile-types@4.0.0

## 2.0.8

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-types@3.2.0

## 2.0.7

### Patch Changes

- @pnpm/lockfile-types@3.1.5

## 2.0.6

### Patch Changes

- @pnpm/lockfile-types@3.1.4

## 2.0.5

### Patch Changes

- @pnpm/lockfile-types@3.1.3

## 2.0.4

### Patch Changes

- @pnpm/lockfile-types@3.1.2

## 2.0.3

### Patch Changes

- @pnpm/lockfile-types@3.1.1

## 2.0.2

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/lockfile-types@3.1.0

## 2.0.1

### Patch Changes

- a1a03d145: Import only the required functions from ramda.

## 2.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [6871d74b2]
  - @pnpm/lockfile-types@3.0.0

## 1.0.1

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/lockfile-types@2.2.0

## 1.0.0

### Major Changes

- 3776b5a52: Initial release.
