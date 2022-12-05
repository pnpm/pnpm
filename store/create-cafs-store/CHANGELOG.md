# @pnpm/create-cafs-store

## 3.0.3

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/cafs@5.0.3
  - @pnpm/fs.indexed-pkg-importer@2.0.2

## 3.0.2

### Patch Changes

- @pnpm/cafs@5.0.2
- @pnpm/fetcher-base@13.1.4
- @pnpm/store-controller-types@14.1.5
- @pnpm/fs.indexed-pkg-importer@2.0.2

## 3.0.1

### Patch Changes

- @pnpm/cafs@5.0.1
- @pnpm/fetcher-base@13.1.3
- @pnpm/store-controller-types@14.1.4
- @pnpm/fs.indexed-pkg-importer@2.0.1

## 3.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/cafs@5.0.0
  - @pnpm/fs.indexed-pkg-importer@2.0.0

## 2.2.5

### Patch Changes

- @pnpm/fs.indexed-pkg-importer@1.1.4

## 2.2.4

### Patch Changes

- @pnpm/cafs@4.3.2
- @pnpm/fetcher-base@13.1.2
- @pnpm/store-controller-types@14.1.3
- @pnpm/fs.indexed-pkg-importer@1.1.3

## 2.2.3

### Patch Changes

- @pnpm/cafs@4.3.1
- @pnpm/fetcher-base@13.1.1
- @pnpm/store-controller-types@14.1.2
- @pnpm/fs.indexed-pkg-importer@1.1.2

## 2.2.2

### Patch Changes

- Updated dependencies [745143e79]
  - @pnpm/cafs@4.3.0
  - @pnpm/fetcher-base@13.1.0
  - @pnpm/store-controller-types@14.1.1
  - @pnpm/fs.indexed-pkg-importer@1.1.1

## 2.2.1

### Patch Changes

- Updated dependencies [dbac0ca01]
  - @pnpm/cafs@4.2.1
  - @pnpm/fs.indexed-pkg-importer@1.1.1

## 2.2.0

### Minor Changes

- 32915f0e4: Refactor cafs types into separate package and add additional properties including `cafsDir` and `getFilePathInCafs`.

### Patch Changes

- Updated dependencies [32915f0e4]
- Updated dependencies [23984abd1]
  - @pnpm/cafs@4.2.0
  - @pnpm/fetcher-base@13.1.0
  - @pnpm/store-controller-types@14.1.1
  - @pnpm/fs.indexed-pkg-importer@1.1.1

## 2.1.1

### Patch Changes

- Updated dependencies [c191ca7bf]
  - @pnpm/cafs@4.1.0
  - @pnpm/fs.indexed-pkg-importer@1.1.0

## 2.1.0

### Minor Changes

- 65c4260de: Support a new hook for passing a custom package importer to the store controller.

### Patch Changes

- Updated dependencies [39c040127]
- Updated dependencies [65c4260de]
  - @pnpm/cafs@4.0.9
  - @pnpm/fs.indexed-pkg-importer@1.1.0
  - @pnpm/store-controller-types@14.1.0

## 2.0.3

### Patch Changes

- @pnpm/cafs@4.0.8
- @pnpm/fetcher-base@13.0.2
- @pnpm/store-controller-types@14.0.2
- @pnpm/fs.indexed-pkg-importer@1.0.1

## 2.0.2

### Patch Changes

- Updated dependencies [7922d6314]
  - @pnpm/fs.indexed-pkg-importer@1.0.0

## 2.0.1

### Patch Changes

- @pnpm/cafs@4.0.7
- @pnpm/core-loggers@7.0.5
- @pnpm/fetcher-base@13.0.1
- @pnpm/store-controller-types@14.0.1

## 2.0.0

### Major Changes

- 2a34b21ce: Rename engine and targetEngine fields to sideEffectsCacheKey.

### Minor Changes

- 47b5e45dd: `package-import-method` supports a new option: `clone-or-copy`.

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/fetcher-base@13.0.0
  - @pnpm/store-controller-types@14.0.0
  - @pnpm/cafs@4.0.6
  - @pnpm/core-loggers@7.0.4

## 1.1.0

### Minor Changes

- 0abfe1718: New optional option added to package importer: `requiresBuild`. When `requiresBuild` is `true`, the package should only be imported using cloning or copying.
- 0abfe1718: New import method added: `clone-or-copy`.

### Patch Changes

- Updated dependencies [0abfe1718]
  - @pnpm/fetcher-base@12.1.0
  - @pnpm/cafs@4.0.5
  - @pnpm/core-loggers@7.0.3
  - @pnpm/store-controller-types@13.0.4

## 1.0.3

### Patch Changes

- @pnpm/cafs@4.0.4
- @pnpm/core-loggers@7.0.2
- @pnpm/fetcher-base@12.0.3
- @pnpm/store-controller-types@13.0.3

## 1.0.2

### Patch Changes

- Updated dependencies [6756c2b02]
  - @pnpm/cafs@4.0.3
  - @pnpm/fetcher-base@12.0.2
  - @pnpm/store-controller-types@13.0.2

## 1.0.1

### Patch Changes

- Updated dependencies [cadefe5b6]
  - @pnpm/cafs@4.0.2

## 1.0.0

### Major Changes

- 1ceb632b1: Project created.

### Patch Changes

- @pnpm/core-loggers@7.0.1
- @pnpm/fetcher-base@12.0.1
- @pnpm/store-controller-types@13.0.1
- @pnpm/cafs@4.0.1
