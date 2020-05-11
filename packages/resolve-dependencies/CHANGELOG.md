# @pnpm/resolve-dependencies

## 15.0.0-alpha.5

### Major Changes

- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- 4cc0ead2: Update replace-string to v3.1.0.
- Updated dependencies [da091c71]
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/core-loggers@4.0.2-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/npm-resolver@7.3.12-alpha.2
  - @pnpm/package-is-installable@4.0.8-alpha.0
  - @pnpm/pick-registry-for-package@1.0.1-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0

## 14.4.5-alpha.4

### Patch Changes

- 0730bb938: Check the existense of a dependency in `node_modules` at the right location.

## 14.4.5-alpha.3

### Patch Changes

- Updated dependencies [5bc033c43]
  - @pnpm/npm-resolver@8.0.0-alpha.1

## 14.4.5-alpha.2

### Patch Changes

- Updated dependencies [42e6490d1]
- Updated dependencies [f453a5f46]
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/npm-resolver@7.3.12-alpha.0

## 14.4.5-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/store-controller-types@8.0.0-alpha.1

## 14.4.5-alpha.0

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/store-controller-types@8.0.0-alpha.0

## 14.4.4

### Patch Changes

- Updated dependencies [907c63a48]
  - @pnpm/lockfile-utils@2.0.11
