# @pnpm/lockfile.settings-checker

## 1001.0.6

### Patch Changes

- @pnpm/lockfile.types@1001.0.5
- @pnpm/crypto.hash@1000.1.1

## 1001.0.5

### Patch Changes

- @pnpm/crypto.hash@1000.1.1

## 1001.0.4

### Patch Changes

- Updated dependencies [daf47e9]
  - @pnpm/crypto.hash@1000.1.0
  - @pnpm/lockfile.types@1001.0.4

## 1001.0.3

### Patch Changes

- @pnpm/lockfile.types@1001.0.3
- @pnpm/crypto.hash@1000.0.0

## 1001.0.2

### Patch Changes

- @pnpm/lockfile.types@1001.0.2
- @pnpm/parse-overrides@1000.0.2
- @pnpm/crypto.hash@1000.0.0

## 1001.0.1

### Patch Changes

- @pnpm/lockfile.types@1001.0.1
- @pnpm/crypto.hash@1000.0.0

## 1001.0.0

### Major Changes

- a76da0c: Removed lockfile conversion from v6 to v9. If you need to convert lockfile v6 to v9, use pnpm CLI v9.

### Minor Changes

- 6483b64: A new setting, `inject-workspace-packages`, has been added to allow hard-linking all local workspace dependencies instead of symlinking them. Previously, this behavior was achievable via the [`dependenciesMeta[].injected`](https://pnpm.io/package_json#dependenciesmetainjected) setting, which remains supported [#8836](https://github.com/pnpm/pnpm/pull/8836).

### Patch Changes

- Updated dependencies [6483b64]
- Updated dependencies [a76da0c]
  - @pnpm/lockfile.types@1001.0.0
  - @pnpm/parse-overrides@1000.0.1
  - @pnpm/crypto.hash@1000.0.0

## 1.0.2

### Patch Changes

- Updated dependencies [dcd2917]
  - @pnpm/crypto.hash@1.0.0
  - @pnpm/parse-overrides@5.1.2

## 1.0.1

### Patch Changes

- @pnpm/crypto.base32-hash@3.0.1

## 1.0.0

### Major Changes

- 51f3ba1: Initial Release
