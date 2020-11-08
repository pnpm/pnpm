# @pnpm/get-context

## 3.3.1

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/lockfile-file@3.1.1
  - @pnpm/read-projects-context@4.0.11

## 3.3.0

### Minor Changes

- 3776b5a52: A new option added to the context: lockfileHadConflicts.

### Patch Changes

- Updated dependencies [3776b5a52]
  - @pnpm/lockfile-file@3.1.0
  - @pnpm/read-projects-context@4.0.10

## 3.2.11

### Patch Changes

- Updated dependencies [dbcc6c96f]
- Updated dependencies [09492b7b4]
  - @pnpm/lockfile-file@3.0.18
  - @pnpm/modules-yaml@8.0.5
  - @pnpm/read-projects-context@4.0.9

## 3.2.10

### Patch Changes

- Updated dependencies [aa6bc4f95]
  - @pnpm/lockfile-file@3.0.17
  - @pnpm/read-projects-context@4.0.8

## 3.2.9

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/lockfile-file@3.0.16
  - @pnpm/core-loggers@5.0.2
  - @pnpm/modules-yaml@8.0.4
  - @pnpm/read-projects-context@4.0.7

## 3.2.8

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - @pnpm/lockfile-file@3.0.15
  - @pnpm/core-loggers@5.0.1
  - @pnpm/modules-yaml@8.0.3
  - @pnpm/read-projects-context@4.0.6

## 3.2.7

### Patch Changes

- ac3042858: When purging an incompatible modules directory, don't remove `.dot_files` that don't belong to pnpm. (<https://github.com/pnpm/pnpm/issues/2506>)

## 3.2.6

### Patch Changes

- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0

## 3.2.5

### Patch Changes

- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/lockfile-file@3.0.14
  - @pnpm/read-projects-context@4.0.5

## 3.2.4

### Patch Changes

- 972864e0d: publicHoistPattern=undefined should be considered to be the same as publicHoistPattern='' (empty string).
- Updated dependencies [9550b0505]
  - @pnpm/lockfile-file@3.0.13
  - @pnpm/read-projects-context@4.0.4

## 3.2.3

### Patch Changes

- 51086e6e4: Fix text in registries mismatch error message.
- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/lockfile-file@3.0.12
  - @pnpm/read-projects-context@4.0.3

## 3.2.2

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - @pnpm/modules-yaml@8.0.2
  - @pnpm/read-projects-context@4.0.2

## 3.2.1

### Patch Changes

- 25b425ca2: When purging an incompatible modules directory, don't remove the actual directory, just the contents of it.

## 3.2.0

### Minor Changes

- a01626668: Add `originalManifest` that stores the unmodified.

## 3.1.0

### Minor Changes

- 9a908bc07: Use `contextLogger` to log `virtualStoreDir`, `storeDir`, and `currentLockfileExists`.

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0

## 3.0.1

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - @pnpm/lockfile-file@3.0.11
  - @pnpm/modules-yaml@8.0.1
  - @pnpm/read-projects-context@4.0.1

## 3.0.0

### Major Changes

- 71a8c8ce3: `hoistedAliases` replaced with `hoistedDependencies`.

  `shamefullyHoist` replaced with `publicHoistPattern`.

  `forceShamefullyHoist` replaced with `forcePublicHoistPattern`.

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/read-projects-context@4.0.0
  - @pnpm/types@6.1.0
  - @pnpm/modules-yaml@8.0.0
  - @pnpm/core-loggers@4.1.1
  - @pnpm/lockfile-file@3.0.10

## 2.1.2

### Patch Changes

- Updated dependencies [2ebb7af33]
  - @pnpm/core-loggers@4.1.0

## 2.1.1

### Patch Changes

- 58c02009f: When checking compatibility of the existing modules directory, start with the layout version. Otherwise, it may happen that some of the fields were renamed and other checks will fail.

## 2.1.0

### Minor Changes

- 327bfbf02: Add `currentLockfileIsUpToDate` to the context.

## 2.0.0

### Major Changes

- 3f73eaf0c: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- 802d145fc: Remove `independent-leaves` support.
- e3990787a: Rename NodeModules to Modules in option names.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [3f73eaf0c]
- Updated dependencies [da091c711]
- Updated dependencies [802d145fc]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/modules-yaml@7.0.0
  - @pnpm/types@6.0.0
  - @pnpm/read-projects-context@3.0.0
  - @pnpm/core-loggers@4.0.2
  - @pnpm/error@1.2.1
  - @pnpm/lockfile-file@3.0.9

## 2.0.0-alpha.2

### Patch Changes

- Updated dependencies [ca9f50844]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/lockfile-file@3.0.9-alpha.2
  - @pnpm/read-projects-context@2.0.2-alpha.2

## 2.0.0-alpha.1

### Major Changes

- 3f73eaf0: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- e3990787: Rename NodeModules to Modules in option names.

### Patch Changes

- Updated dependencies [3f73eaf0]
- Updated dependencies [da091c71]
  - @pnpm/modules-yaml@7.0.0-alpha.0
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/read-projects-context@2.0.2-alpha.1
  - @pnpm/core-loggers@4.0.2-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.1

## 1.2.2-alpha.0

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.0
  - @pnpm/read-projects-context@2.0.2-alpha.0

## 1.2.1

### Patch Changes

- 907c63a48: Update dependencies.
- 907c63a48: Use `fs.mkdir` instead of `make-dir`.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/lockfile-file@3.0.8
  - @pnpm/modules-yaml@6.0.2
  - @pnpm/read-projects-context@2.0.1
