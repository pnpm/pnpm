# @pnpm/store-controller-types

## 9.2.0

### Minor Changes

- 8698a7060: New option added: preferWorkspacePackages. When it is `true`, dependencies are linked from the workspace even, when there are newer version available in the registry.

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/resolver-base@7.1.0

## 9.1.2

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/resolver-base@7.0.5

## 9.1.1

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/resolver-base@7.0.4

## 9.1.0

### Minor Changes

- 0a6544043: A new field added to the package files index: `checkedAt`. `checkedAt` is the timestamp (number of milliseconds), when the file's content was verified the last time.

## 9.0.0

### Major Changes

- 86cd72de3: The `importPackage` function of the store controller returns the `importMethod` that was used to link the package to the virtual store. If importing was not needed, `importMethod` is `undefined`.

## 8.0.2

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/resolver-base@7.0.3

## 8.0.1

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/resolver-base@7.0.2

## 8.0.0

### Major Changes

- 16d1ac0fd: `body.cacheByEngine` removed from `PackageResponse`.
- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- b6a82072e: Using a content-addressable filesystem for storing packages.
- 802d145fc: `getPackageLocation()` removed from store. Remove `inStoreLocation` from the result of `fetchPackage()`.
- a5febb913: The importPackage function of the store controller is importing packages directly from the side-effects cache.
- a5febb913: The upload function of the store controller accepts `opts.filesIndexFile` instead of `opts.packageId`.

### Minor Changes

- f516d266c: Executables are saved into a separate directory inside the content-addressable storage.
- 42e6490d1: The fetch package to store function does not need the pkgName anymore.
- a5febb913: Package request response contains the path to the files index file.
- a5febb913: sideEffects property added to files index file.

### Patch Changes

- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0
  - @pnpm/resolver-base@7.0.1

## 8.0.0-alpha.4

### Major Changes

- 16d1ac0fd: `body.cacheByEngine` removed from `PackageResponse`.
- a5febb913: The importPackage function of the store controller is importing packages directly from the side-effects cache.
- a5febb913: The upload function of the store controller accepts `opts.filesIndexFile` instead of `opts.packageId`.

### Minor Changes

- a5febb913: Package request response contains the path to the files index file.
- a5febb913: sideEffects property added to files index file.

## 8.0.0-alpha.3

### Major Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0

## 8.0.0-alpha.2

### Minor Changes

- 42e6490d1: The fetch package to store function does not need the pkgName anymore.

## 8.0.0-alpha.1

### Minor Changes

- 4f62d0383: Executables are saved into a separate directory inside the content-addressable storage.

## 8.0.0-alpha.0

### Major Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.
