# @pnpm/tarball-resolver

## 1002.1.2

### Patch Changes

- Updated dependencies [86b33e9]
- Updated dependencies [d1edf73]
- Updated dependencies [f91922c]
  - @pnpm/resolver-base@1005.0.0

## 1002.1.1

### Patch Changes

- Updated dependencies [1ba2e15]
- Updated dependencies [1a07b8f]
  - @pnpm/fetching-types@1000.2.0
  - @pnpm/resolver-base@1004.1.0

## 1002.1.0

### Minor Changes

- 2721291: Create different resolver result types which provide more information.

### Patch Changes

- Updated dependencies [2721291]
- Updated dependencies [6acf819]
  - @pnpm/resolver-base@1004.0.0

## 1002.0.2

### Patch Changes

- c307634: Dependencies specified via a URL that redirects will only be locked to the target if it is immutable, fixing a regression when installing from GitHub releases. ([#9531](https://github.com/pnpm/pnpm/issues/9531))

## 1002.0.1

### Patch Changes

- @pnpm/resolver-base@1003.0.1

## 1002.0.0

### Major Changes

- 8a9f3a4: `pref` renamed to `bareSpecifier`.
- 5b73df1: Renamed `normalizedPref` to `specifiers`.

### Patch Changes

- Updated dependencies [8a9f3a4]
- Updated dependencies [5b73df1]
- Updated dependencies [9c3dd03]
  - @pnpm/resolver-base@1003.0.0

## 1001.0.8

### Patch Changes

- Updated dependencies [81f441c]
  - @pnpm/resolver-base@1002.0.0

## 1001.0.7

### Patch Changes

- Updated dependencies [72cff38]
  - @pnpm/resolver-base@1001.0.0

## 1001.0.6

### Patch Changes

- @pnpm/resolver-base@1000.2.1

## 1001.0.5

### Patch Changes

- Updated dependencies [3d52365]
  - @pnpm/resolver-base@1000.2.0

## 1001.0.4

### Patch Changes

- @pnpm/resolver-base@1000.1.4

## 1001.0.3

### Patch Changes

- @pnpm/resolver-base@1000.1.3

## 1001.0.2

### Patch Changes

- @pnpm/resolver-base@1000.1.2

## 1001.0.1

### Patch Changes

- @pnpm/resolver-base@1000.1.1

## 1001.0.0

### Major Changes

- b0f3c71: Dependencies specified via a URL are now recorded in the lockfile using their final resolved URL. Thus, if the original URL redirects, the final redirect target will be saved in the lockfile [#8833](https://github.com/pnpm/pnpm/issues/8833).

### Patch Changes

- Updated dependencies [6483b64]
- Updated dependencies [b0f3c71]
  - @pnpm/resolver-base@1000.1.0
  - @pnpm/fetching-types@1000.1.0

## 9.0.8

### Patch Changes

- 3be45b7: Fix `ERR_PNPM_TARBALL_EXTRACT` error while installing a dependency from GitHub having a slash in branch name [#7697](https://github.com/pnpm/pnpm/issues/7697).

## 9.0.7

### Patch Changes

- @pnpm/resolver-base@13.0.4

## 9.0.6

### Patch Changes

- @pnpm/resolver-base@13.0.3

## 9.0.5

### Patch Changes

- @pnpm/resolver-base@13.0.2

## 9.0.4

### Patch Changes

- @pnpm/resolver-base@13.0.1

## 9.0.3

### Patch Changes

- Updated dependencies [dd00eeb]
  - @pnpm/resolver-base@13.0.0

## 9.0.2

### Patch Changes

- @pnpm/resolver-base@12.0.2

## 9.0.1

### Patch Changes

- @pnpm/resolver-base@12.0.1

## 9.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [43cdd87]
- Updated dependencies [b13d2dc]
  - @pnpm/resolver-base@12.0.0

## 8.0.8

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/resolver-base@11.1.0

## 8.0.7

### Patch Changes

- @pnpm/resolver-base@11.0.2

## 8.0.6

### Patch Changes

- @pnpm/resolver-base@11.0.1

## 8.0.5

### Patch Changes

- Updated dependencies [4c2450208]
  - @pnpm/resolver-base@11.0.0

## 8.0.4

### Patch Changes

- @pnpm/resolver-base@10.0.4

## 8.0.3

### Patch Changes

- @pnpm/resolver-base@10.0.3

## 8.0.2

### Patch Changes

- @pnpm/resolver-base@10.0.2

## 8.0.1

### Patch Changes

- @pnpm/resolver-base@10.0.1

## 8.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/resolver-base@10.0.0

## 7.0.4

### Patch Changes

- Updated dependencies [029143cff]
- Updated dependencies [029143cff]
  - @pnpm/resolver-base@9.2.0

## 7.0.3

### Patch Changes

- @pnpm/resolver-base@9.1.5

## 7.0.2

### Patch Changes

- @pnpm/resolver-base@9.1.4

## 7.0.1

### Patch Changes

- @pnpm/resolver-base@9.1.3

## 7.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

## 6.0.9

### Patch Changes

- @pnpm/resolver-base@9.1.2

## 6.0.8

### Patch Changes

- @pnpm/resolver-base@9.1.1

## 6.0.7

### Patch Changes

- Updated dependencies [23984abd1]
  - @pnpm/resolver-base@9.1.0

## 6.0.6

### Patch Changes

- @pnpm/resolver-base@9.0.6

## 6.0.5

### Patch Changes

- @pnpm/resolver-base@9.0.5

## 6.0.4

### Patch Changes

- @pnpm/resolver-base@9.0.4

## 6.0.3

### Patch Changes

- @pnpm/resolver-base@9.0.3

## 6.0.2

### Patch Changes

- @pnpm/resolver-base@9.0.2

## 6.0.1

### Patch Changes

- @pnpm/resolver-base@9.0.1

## 6.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/resolver-base@9.0.0

## 5.0.11

### Patch Changes

- @pnpm/resolver-base@8.1.6

## 5.0.10

### Patch Changes

- @pnpm/resolver-base@8.1.5

## 5.0.9

### Patch Changes

- @pnpm/resolver-base@8.1.4

## 5.0.8

### Patch Changes

- @pnpm/resolver-base@8.1.3

## 5.0.7

### Patch Changes

- @pnpm/resolver-base@8.1.2

## 5.0.6

### Patch Changes

- @pnpm/resolver-base@8.1.1

## 5.0.5

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/resolver-base@8.1.0

## 5.0.4

### Patch Changes

- @pnpm/resolver-base@8.0.4

## 5.0.3

### Patch Changes

- @pnpm/resolver-base@8.0.3

## 5.0.2

### Patch Changes

- @pnpm/resolver-base@8.0.2

## 5.0.1

### Patch Changes

- @pnpm/resolver-base@8.0.1

## 5.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- 992820161: The ID of a tarball dependency should not contain colons, when the URL has a port. The colon should be escaped with a plus sign.
- Updated dependencies [97b986fbc]
  - @pnpm/resolver-base@8.0.0

## 4.0.8

### Patch Changes

- a00ee0035: The ID of a tarball dependency should not contain colons, when the URL has a port. The colon should be escaped with a plus sign.

## 4.0.7

### Patch Changes

- @pnpm/resolver-base@7.1.1

## 4.0.6

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/resolver-base@7.1.0

## 4.0.5

### Patch Changes

- @pnpm/resolver-base@7.0.5

## 4.0.4

### Patch Changes

- @pnpm/resolver-base@7.0.4

## 4.0.3

### Patch Changes

- 83b146d63: Ignore URLs to repositories.

## 4.0.2

### Patch Changes

- @pnpm/resolver-base@7.0.3

## 4.0.1

### Patch Changes

- @pnpm/resolver-base@7.0.2

## 4.0.0

### Major Changes

- 41d92948b: The direct tarball dependency ID starts with a @ and the tarball extension is not removed.

## 3.0.5

### Patch Changes

- @pnpm/resolver-base@7.0.1

## 3.0.5-alpha.0

### Patch Changes

- @pnpm/resolver-base@7.0.1-alpha.0
