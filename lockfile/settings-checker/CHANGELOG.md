# @pnpm/lockfile.settings-checker

## 1100.0.2

### Patch Changes

- @pnpm/lockfile.types@1100.0.2
- @pnpm/lockfile.verification@1100.0.2
- @pnpm/crypto.hash@1100.0.0

## 1100.0.1

### Patch Changes

- @pnpm/lockfile.types@1100.0.1
- @pnpm/lockfile.verification@1100.0.1
- @pnpm/crypto.hash@1100.0.0

## 1002.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- 606f53e: Added a new `dedupePeers` setting that reduces peer dependency duplication. When enabled, peer dependency suffixes use version-only identifiers (`name@version`) instead of full dep paths, eliminating nested suffixes like `(foo@1.0.0(bar@2.0.0))`. This dramatically reduces the number of package instances in projects with many recursive peer dependencies [#11070](https://github.com/pnpm/pnpm/issues/11070).

### Patch Changes

- 69ebe38: Properly throw a frozen lockfile error when changing catalogs defined in `pnpm-workspace.yaml` and running `pnpm install --frozen-lockfile`. This previously passed silently as reported in [#9369](https://github.com/pnpm/pnpm/issues/9369).
- Updated dependencies [a8f016c]
- Updated dependencies [606f53e]
- Updated dependencies [491a84f]
- Updated dependencies [521e4a6]
- Updated dependencies [7d2fd48]
- Updated dependencies [50fbeca]
- Updated dependencies [69ebe38]
- Updated dependencies [38b8e35]
  - @pnpm/lockfile.types@1003.0.0
  - @pnpm/config.parse-overrides@1002.0.0
  - @pnpm/lockfile.verification@1002.0.0
  - @pnpm/catalogs.types@1001.0.0
  - @pnpm/crypto.hash@1001.0.0

## 1001.0.16

### Patch Changes

- @pnpm/lockfile.types@1002.0.2
- @pnpm/crypto.hash@1000.2.1

## 1001.0.15

### Patch Changes

- @pnpm/crypto.hash@1000.2.1

## 1001.0.14

### Patch Changes

- @pnpm/parse-overrides@1001.0.3
- @pnpm/crypto.hash@1000.2.0

## 1001.0.13

### Patch Changes

- @pnpm/lockfile.types@1002.0.1
- @pnpm/crypto.hash@1000.2.0

## 1001.0.12

### Patch Changes

- Updated dependencies [d1edf73]
- Updated dependencies [f91922c]
  - @pnpm/lockfile.types@1002.0.0
  - @pnpm/parse-overrides@1001.0.2
  - @pnpm/crypto.hash@1000.2.0

## 1001.0.11

### Patch Changes

- Updated dependencies [1a07b8f]
  - @pnpm/lockfile.types@1001.1.0
  - @pnpm/crypto.hash@1000.2.0
  - @pnpm/parse-overrides@1001.0.1

## 1001.0.10

### Patch Changes

- Updated dependencies [cf630a8]
  - @pnpm/crypto.hash@1000.2.0

## 1001.0.9

### Patch Changes

- @pnpm/lockfile.types@1001.0.8
- @pnpm/crypto.hash@1000.1.1

## 1001.0.8

### Patch Changes

- Updated dependencies [8a9f3a4]
  - @pnpm/parse-overrides@1001.0.0
  - @pnpm/lockfile.types@1001.0.7
  - @pnpm/crypto.hash@1000.1.1

## 1001.0.7

### Patch Changes

- @pnpm/lockfile.types@1001.0.6
- @pnpm/crypto.hash@1000.1.1

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
