# @pnpm/modules-cleaner

## 9.0.0-alpha.4

### Major Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Minor Changes

- 7179cc56: Don't try to remove empty branches of a directory tree, when pruning `node_modules`.

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/core-loggers@4.0.2-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/filter-lockfile@3.2.3-alpha.1
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/package-bins@4.0.4-alpha.0
  - @pnpm/read-package-json@3.1.1-alpha.0

## 9.0.0-alpha.3

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- @pnpm/filter-lockfile@3.2.3-alpha.0

## 8.0.17-alpha.2

### Patch Changes

- Updated dependencies [42e6490d1]
  - @pnpm/store-controller-types@8.0.0-alpha.2

## 8.0.17-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/store-controller-types@8.0.0-alpha.1

## 8.0.17-alpha.0

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/store-controller-types@8.0.0-alpha.0

## 8.0.16

### Patch Changes

- Updated dependencies [907c63a48]
  - @pnpm/filter-lockfile@3.2.2
  - @pnpm/lockfile-utils@2.0.11
