# @pnpm/fs.indexed-pkg-importer

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
