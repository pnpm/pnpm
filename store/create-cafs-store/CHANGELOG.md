# @pnpm/create-cafs-store

## 7.0.4

### Patch Changes

- @pnpm/fetcher-base@16.0.3
- @pnpm/store-controller-types@18.1.2
- @pnpm/exec.pkg-requires-build@1.0.3
- @pnpm/store.cafs@3.0.4
- @pnpm/fs.indexed-pkg-importer@6.0.4

## 7.0.3

### Patch Changes

- @pnpm/exec.pkg-requires-build@1.0.2
- @pnpm/fetcher-base@16.0.2
- @pnpm/store.cafs@3.0.3
- @pnpm/store-controller-types@18.1.1
- @pnpm/fs.indexed-pkg-importer@6.0.3

## 7.0.2

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/store-controller-types@18.1.0
  - @pnpm/fs.indexed-pkg-importer@6.0.2
  - @pnpm/store.cafs@3.0.2

## 7.0.1

### Patch Changes

- @pnpm/exec.pkg-requires-build@1.0.1
- @pnpm/fetcher-base@16.0.1
- @pnpm/store.cafs@3.0.1
- @pnpm/store-controller-types@18.0.1
- @pnpm/fs.indexed-pkg-importer@6.0.1

## 7.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- 730929e: Add a field named `ignoredOptionalDependencies`. This is an array of strings. If an optional dependency has its name included in this array, it will be skipped.

### Patch Changes

- Updated dependencies [43cdd87]
- Updated dependencies [6cdbf11]
- Updated dependencies [36dcaa0]
- Updated dependencies [0e6b757]
- Updated dependencies [730929e]
  - @pnpm/store-controller-types@18.0.0
  - @pnpm/fs.indexed-pkg-importer@6.0.0
  - @pnpm/fetcher-base@16.0.0
  - @pnpm/store.cafs@3.0.0
  - @pnpm/exec.pkg-requires-build@1.0.0

## 6.0.13

### Patch Changes

- Updated dependencies [31054a63e]
- Updated dependencies [e2e08b98f]
- Updated dependencies [df9b16aa9]
  - @pnpm/store-controller-types@17.2.0
  - @pnpm/fs.indexed-pkg-importer@5.0.13
  - @pnpm/store.cafs@2.0.12
  - @pnpm/fetcher-base@15.0.7

## 6.0.12

### Patch Changes

- Updated dependencies [19be6b704]
  - @pnpm/fs.indexed-pkg-importer@5.0.12

## 6.0.11

### Patch Changes

- Updated dependencies [33313d2fd]
  - @pnpm/fs.indexed-pkg-importer@5.0.11
  - @pnpm/store.cafs@2.0.11
  - @pnpm/fetcher-base@15.0.6
  - @pnpm/store-controller-types@17.1.4

## 6.0.10

### Patch Changes

- @pnpm/fetcher-base@15.0.5
- @pnpm/store.cafs@2.0.10
- @pnpm/store-controller-types@17.1.3
- @pnpm/fs.indexed-pkg-importer@5.0.10

## 6.0.9

### Patch Changes

- Updated dependencies [418866ac0]
  - @pnpm/fs.indexed-pkg-importer@5.0.9

## 6.0.8

### Patch Changes

- Updated dependencies [291607c5a]
  - @pnpm/store-controller-types@17.1.2
  - @pnpm/fs.indexed-pkg-importer@5.0.8
  - @pnpm/store.cafs@2.0.9

## 6.0.7

### Patch Changes

- cfc017ee3: Optional dependencies that do not have to be built will be reflinked (or hardlinked) to the store instead of copied [#7046](https://github.com/pnpm/pnpm/issues/7046).
- Updated dependencies [7ea45afbe]
- Updated dependencies [cfc017ee3]
  - @pnpm/store-controller-types@17.1.1
  - @pnpm/exec.files-include-install-scripts@1.0.0
  - @pnpm/fetcher-base@15.0.4
  - @pnpm/fs.indexed-pkg-importer@5.0.7
  - @pnpm/store.cafs@2.0.8

## 6.0.6

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/store-controller-types@17.1.0
  - @pnpm/fs.indexed-pkg-importer@5.0.6
  - @pnpm/store.cafs@2.0.7
  - @pnpm/fetcher-base@15.0.3

## 6.0.5

### Patch Changes

- Updated dependencies [2ca756fd2]
  - @pnpm/fs.indexed-pkg-importer@5.0.5

## 6.0.4

### Patch Changes

- Updated dependencies [01bc58e2c]
- Updated dependencies [6dfbca86b]
  - @pnpm/store.cafs@2.0.6
  - @pnpm/fs.indexed-pkg-importer@5.0.4

## 6.0.3

### Patch Changes

- Updated dependencies [e19de6a59]
  - @pnpm/fs.indexed-pkg-importer@5.0.3

## 6.0.2

### Patch Changes

- Updated dependencies [6337dcdbc]
  - @pnpm/fs.indexed-pkg-importer@5.0.2

## 6.0.1

### Patch Changes

- Updated dependencies [ee6e0734e]
  - @pnpm/fs.indexed-pkg-importer@5.0.1
  - @pnpm/fetcher-base@15.0.2
  - @pnpm/store.cafs@2.0.5
  - @pnpm/store-controller-types@17.0.1

## 6.0.0

### Major Changes

- 9caa33d53: `fromStore` replaced with `resolvedFrom`.

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/store-controller-types@17.0.0
  - @pnpm/fs.indexed-pkg-importer@5.0.0
  - @pnpm/store.cafs@2.0.4
  - @pnpm/fetcher-base@15.0.1

## 5.1.1

### Patch Changes

- Updated dependencies [cb6e4212c]
  - @pnpm/fs.indexed-pkg-importer@4.1.1

## 5.1.0

### Minor Changes

- 03cdccc6e: New option added: disableRelinkFromStore.

### Patch Changes

- Updated dependencies [03cdccc6e]
  - @pnpm/store-controller-types@16.1.0
  - @pnpm/fs.indexed-pkg-importer@4.1.0
  - @pnpm/store.cafs@2.0.3
  - @pnpm/fetcher-base@15.0.1

## 5.0.2

### Patch Changes

- Updated dependencies [b3947185c]
  - @pnpm/store.cafs@2.0.2
  - @pnpm/fs.indexed-pkg-importer@4.0.1

## 5.0.1

### Patch Changes

- Updated dependencies [b548f2f43]
- Updated dependencies [4a1a9431d]
  - @pnpm/store.cafs@2.0.1
  - @pnpm/fetcher-base@15.0.1
  - @pnpm/store-controller-types@16.0.1
  - @pnpm/fs.indexed-pkg-importer@4.0.1

## 5.0.0

### Major Changes

- f2009d175: Import packages synchronously.
- 083bbf590: Breaking changes to the API.

### Patch Changes

- Updated dependencies [0fd9e6a6c]
- Updated dependencies [f2009d175]
- Updated dependencies [494f87544]
- Updated dependencies [70b2830ac]
- Updated dependencies [083bbf590]
- Updated dependencies [083bbf590]
  - @pnpm/store.cafs@2.0.0
  - @pnpm/fs.indexed-pkg-importer@4.0.0
  - @pnpm/store-controller-types@16.0.0
  - @pnpm/fetcher-base@15.0.0

## 4.0.8

### Patch Changes

- Updated dependencies [73f2b6826]
  - @pnpm/store.cafs@1.0.2
  - @pnpm/fs.indexed-pkg-importer@3.0.2

## 4.0.7

### Patch Changes

- Updated dependencies [fe1c5f48d]
  - @pnpm/store.cafs@1.0.1
  - @pnpm/fs.indexed-pkg-importer@3.0.2

## 4.0.6

### Patch Changes

- Updated dependencies [4bbf482d1]
  - @pnpm/store.cafs@1.0.0
  - @pnpm/fs.indexed-pkg-importer@3.0.2

## 4.0.5

### Patch Changes

- Updated dependencies [250f7e9fe]
- Updated dependencies [e958707b2]
  - @pnpm/cafs@7.0.5
  - @pnpm/fs.indexed-pkg-importer@3.0.2
  - @pnpm/fetcher-base@14.0.2
  - @pnpm/store-controller-types@15.0.2

## 4.0.4

### Patch Changes

- Updated dependencies [b81cefdcd]
  - @pnpm/cafs@7.0.4
  - @pnpm/fs.indexed-pkg-importer@3.0.1

## 4.0.3

### Patch Changes

- Updated dependencies [e57e2d340]
  - @pnpm/cafs@7.0.3
  - @pnpm/fs.indexed-pkg-importer@3.0.1

## 4.0.2

### Patch Changes

- Updated dependencies [d55b41a8b]
- Updated dependencies [614d5bd72]
  - @pnpm/cafs@7.0.2
  - @pnpm/fs.indexed-pkg-importer@3.0.1

## 4.0.1

### Patch Changes

- @pnpm/fetcher-base@14.0.1
- @pnpm/cafs@7.0.1
- @pnpm/store-controller-types@15.0.1
- @pnpm/fs.indexed-pkg-importer@3.0.1

## 4.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/store-controller-types@15.0.0
  - @pnpm/fs.indexed-pkg-importer@3.0.0
  - @pnpm/fetcher-base@14.0.0
  - @pnpm/cafs@7.0.0

## 3.1.6

### Patch Changes

- Updated dependencies [955874422]
  - @pnpm/fs.indexed-pkg-importer@2.1.4
  - @pnpm/cafs@6.0.2

## 3.1.5

### Patch Changes

- @pnpm/fetcher-base@13.1.6
- @pnpm/store-controller-types@14.3.1
- @pnpm/fs.indexed-pkg-importer@2.1.3
- @pnpm/cafs@6.0.1

## 3.1.4

### Patch Changes

- Updated dependencies [78d4cf1f7]
  - @pnpm/fs.indexed-pkg-importer@2.1.2

## 3.1.3

### Patch Changes

- Updated dependencies [98d6603f3]
- Updated dependencies [98d6603f3]
  - @pnpm/cafs@6.0.0
  - @pnpm/fs.indexed-pkg-importer@2.1.1

## 3.1.2

### Patch Changes

- Updated dependencies [1e6de89b6]
  - @pnpm/cafs@5.0.6
  - @pnpm/fs.indexed-pkg-importer@2.1.1

## 3.1.1

### Patch Changes

- Updated dependencies [891a8d763]
- Updated dependencies [c7b05cd9a]
  - @pnpm/store-controller-types@14.3.0
  - @pnpm/fs.indexed-pkg-importer@2.1.1
  - @pnpm/cafs@5.0.5

## 3.1.0

### Minor Changes

- 2458741fa: A new option added to package importer for keeping modules directory: `keepModulesDir`. When this is set to true, if a package already exist at the target location and it has a node_modules directory, then that node_modules directory is moved to the newly imported dependency. This is only needed when node-linker=hoisted is used.

### Patch Changes

- Updated dependencies [2458741fa]
  - @pnpm/fs.indexed-pkg-importer@2.1.0
  - @pnpm/store-controller-types@14.2.0
  - @pnpm/fetcher-base@13.1.5
  - @pnpm/cafs@5.0.4

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
