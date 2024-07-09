# @pnpm/calc-dep-state

## 7.0.3

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0
  - @pnpm/lockfile-utils@11.0.3
  - @pnpm/lockfile-types@7.1.2
  - @pnpm/dependency-path@5.1.2

## 7.0.2

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/lockfile-types@7.1.1
  - @pnpm/lockfile-utils@11.0.2
  - @pnpm/dependency-path@5.1.1

## 7.0.1

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/dependency-path@5.1.0
  - @pnpm/lockfile-types@7.1.0
  - @pnpm/lockfile-utils@11.0.1

## 7.0.0

### Major Changes

- Breaking changes to the API.

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-types@7.0.0
  - @pnpm/lockfile-utils@11.0.0
  - @pnpm/dependency-path@5.0.0

## 6.0.1

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/dependency-path@4.0.0

## 6.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [cdd8365]
- Updated dependencies [c692f80]
- Updated dependencies [89b396b]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [d636eed]
- Updated dependencies [27a96a8]
- Updated dependencies [730929e]
- Updated dependencies [98a1266]
  - @pnpm/dependency-path@3.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/lockfile-types@6.0.0
  - @pnpm/crypto.object-hasher@2.0.0

## 5.0.0

### Major Changes

- 0c383327e: Reduce the length of the side-effects cache key. Instead of saving a stringified object composed from the dependency versions of the package, use the hash calculated from the said object [#7563](https://github.com/pnpm/pnpm/pull/7563).

### Patch Changes

- Updated dependencies [0c383327e]
  - @pnpm/crypto.object-hasher@1.0.0

## 4.1.5

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/lockfile-types@5.1.5
  - @pnpm/dependency-path@2.1.7

## 4.1.4

### Patch Changes

- Updated dependencies
  - @pnpm/lockfile-types@5.1.4
  - @pnpm/dependency-path@2.1.6

## 4.1.3

### Patch Changes

- @pnpm/lockfile-types@5.1.3
- @pnpm/dependency-path@2.1.5

## 4.1.2

### Patch Changes

- @pnpm/lockfile-types@5.1.2
- @pnpm/dependency-path@2.1.4

## 4.1.1

### Patch Changes

- @pnpm/lockfile-types@5.1.1
- @pnpm/dependency-path@2.1.3

## 4.1.0

### Minor Changes

- 16bbac8d5: Add `lockfileToDepGraph` function.

## 4.0.2

### Patch Changes

- Updated dependencies [302ebffc5]
  - @pnpm/constants@7.1.1

## 4.0.1

### Patch Changes

- Updated dependencies [9c4ae87bd]
  - @pnpm/constants@7.1.0

## 4.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/constants@7.0.0

## 3.0.2

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/constants@6.2.0

## 3.0.1

### Patch Changes

- 285ff09ba: Calculate the cache key differently when scripts are ignored.

## 3.0.0

### Major Changes

- 2a34b21ce: Changed the order of arguments in calcDepState and added an optional last argument for patchFileHash.

## 2.0.1

### Patch Changes

- Updated dependencies [1267e4eff]
  - @pnpm/constants@6.1.0

## 2.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/constants@6.0.0

## 1.0.0

### Major Changes

- 1cadc231a: Initial release.
