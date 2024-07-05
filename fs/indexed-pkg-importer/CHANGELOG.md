# @pnpm/fs.indexed-pkg-importer

## 6.0.4

### Patch Changes

- @pnpm/store-controller-types@18.1.2
- @pnpm/core-loggers@10.0.3

## 6.0.3

### Patch Changes

- @pnpm/core-loggers@10.0.2
- @pnpm/store-controller-types@18.1.1

## 6.0.2

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/store-controller-types@18.1.0

## 6.0.1

### Patch Changes

- @pnpm/core-loggers@10.0.1
- @pnpm/store-controller-types@18.0.1

## 6.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [43cdd87]
  - @pnpm/store-controller-types@18.0.0
  - @pnpm/core-loggers@10.0.0
  - @pnpm/graceful-fs@4.0.0

## 5.0.13

### Patch Changes

- e2e08b98f: Prefer hard links over reflinks on Windows as they perform better [#7564](https://github.com/pnpm/pnpm/pull/7564).
- df9b16aa9: Don't fail in Windows CoW if the file already exists [#7554](https://github.com/pnpm/pnpm/issues/7554).
- Updated dependencies [31054a63e]
  - @pnpm/store-controller-types@17.2.0

## 5.0.12

### Patch Changes

- 19be6b704: Fix copy-on-write on Windows Dev Drives [#7468](https://github.com/pnpm/pnpm/issues/7468).

## 5.0.11

### Patch Changes

- 33313d2fd: Update rename-overwrite to v5.
  - @pnpm/core-loggers@9.0.6
  - @pnpm/store-controller-types@17.1.4

## 5.0.10

### Patch Changes

- @pnpm/core-loggers@9.0.5
- @pnpm/store-controller-types@17.1.3

## 5.0.9

### Patch Changes

- 418866ac0: Bump version of `@reflink/reflink` that avoids empty cloned files when using Copy-on-Write on Windows Dev Drives. (#7186)

## 5.0.8

### Patch Changes

- Updated dependencies [291607c5a]
  - @pnpm/store-controller-types@17.1.2

## 5.0.7

### Patch Changes

- Updated dependencies [7ea45afbe]
  - @pnpm/store-controller-types@17.1.1

## 5.0.6

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/store-controller-types@17.1.0
  - @pnpm/core-loggers@9.0.4

## 5.0.5

### Patch Changes

- 2ca756fd2: Don't use reflink on Windows [#7186](https://github.com/pnpm/pnpm/issues/7186).

## 5.0.4

### Patch Changes

- 6dfbca86b: Update reflink.

## 5.0.3

### Patch Changes

- e19de6a59: Fix file cloning to `node_modules` on Windows Dev Drives [#7186](https://github.com/pnpm/pnpm/issues/7186). This is a fix to a regression that was shipped with v8.9.0.

## 5.0.2

### Patch Changes

- 6337dcdbc: Don't fail on reflink creation while importing a package, if the target file already exists.

## 5.0.1

### Patch Changes

- ee6e0734e: Use reflinks instead of hard links by default on macOS and Windows Dev Drives [#5001](https://github.com/pnpm/pnpm/issues/5001).
  - @pnpm/core-loggers@9.0.3
  - @pnpm/store-controller-types@17.0.1

## 5.0.0

### Major Changes

- 9caa33d53: `fromStore` replaced with `resolvedFrom`.

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/store-controller-types@17.0.0
  - @pnpm/graceful-fs@3.2.0

## 4.1.1

### Patch Changes

- cb6e4212c: Verify the existence of the package in node_modules, when disableRelinkFromStore is set to true.

## 4.1.0

### Minor Changes

- 03cdccc6e: New option added: disableRelinkFromStore.

### Patch Changes

- Updated dependencies [03cdccc6e]
  - @pnpm/store-controller-types@16.1.0

## 4.0.1

### Patch Changes

- @pnpm/store-controller-types@16.0.1

## 4.0.0

### Major Changes

- f2009d175: Import packages synchronously.

### Patch Changes

- Updated dependencies [494f87544]
- Updated dependencies [083bbf590]
  - @pnpm/store-controller-types@16.0.0
  - @pnpm/graceful-fs@3.1.0

## 3.0.2

### Patch Changes

- e958707b2: Improve performance by removing cryptographically generated id from temporary file names.
  - @pnpm/core-loggers@9.0.2
  - @pnpm/store-controller-types@15.0.2

## 3.0.1

### Patch Changes

- @pnpm/core-loggers@9.0.1
- @pnpm/store-controller-types@15.0.1

## 3.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/store-controller-types@15.0.0
  - @pnpm/core-loggers@9.0.0
  - @pnpm/graceful-fs@3.0.0

## 2.1.4

### Patch Changes

- 955874422: Retry copying file on EBUSY error [#6201](https://github.com/pnpm/pnpm/issues/6201).
- Updated dependencies [955874422]
  - @pnpm/graceful-fs@2.1.0

## 2.1.3

### Patch Changes

- @pnpm/store-controller-types@14.3.1

## 2.1.2

### Patch Changes

- 78d4cf1f7: Fix "cross-device link not permitted" error when `node-linker` is set to `hoisted` [#5992](https://github.com/pnpm/pnpm/issues/5992).

## 2.1.1

### Patch Changes

- Updated dependencies [891a8d763]
- Updated dependencies [c7b05cd9a]
  - @pnpm/store-controller-types@14.3.0

## 2.1.0

### Minor Changes

- 2458741fa: A new option added to package importer for keeping modules directory: `keepModulesDir`. When this is set to true, if a package already exist at the target location and it has a node_modules directory, then that node_modules directory is moved to the newly imported dependency. This is only needed when node-linker=hoisted is used.

### Patch Changes

- Updated dependencies [2458741fa]
  - @pnpm/store-controller-types@14.2.0
  - @pnpm/core-loggers@8.0.3

## 2.0.2

### Patch Changes

- @pnpm/core-loggers@8.0.2
- @pnpm/store-controller-types@14.1.5

## 2.0.1

### Patch Changes

- @pnpm/core-loggers@8.0.1
- @pnpm/store-controller-types@14.1.4

## 2.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [f884689e0]
  - @pnpm/core-loggers@8.0.0

## 1.1.4

### Patch Changes

- Updated dependencies [3ae888c28]
  - @pnpm/core-loggers@7.1.0

## 1.1.3

### Patch Changes

- @pnpm/core-loggers@7.0.8
- @pnpm/store-controller-types@14.1.3

## 1.1.2

### Patch Changes

- @pnpm/core-loggers@7.0.7
- @pnpm/store-controller-types@14.1.2

## 1.1.1

### Patch Changes

- Updated dependencies [32915f0e4]
  - @pnpm/store-controller-types@14.1.1

## 1.1.0

### Minor Changes

- 65c4260de: Support a new hook for passing a custom package importer to the store controller.

### Patch Changes

- Updated dependencies [65c4260de]
  - @pnpm/store-controller-types@14.1.0

## 1.0.1

### Patch Changes

- @pnpm/core-loggers@7.0.6

## 1.0.0

### Major Changes

- 7922d6314: Initial release.
