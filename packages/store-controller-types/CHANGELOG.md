# @pnpm/store-controller-types

## 14.1.1

### Patch Changes

- 32915f0e4: Refactor cafs types into separate package and add additional properties including `cafsDir` and `getFilePathInCafs`.
- Updated dependencies [32915f0e4]
- Updated dependencies [23984abd1]
  - @pnpm/fetcher-base@13.1.0
  - @pnpm/resolver-base@9.1.0

## 14.1.0

### Minor Changes

- 65c4260de: Support a new hook for passing a custom package importer to the store controller.

## 14.0.2

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/fetcher-base@13.0.2
  - @pnpm/resolver-base@9.0.6

## 14.0.1

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/fetcher-base@13.0.1
  - @pnpm/resolver-base@9.0.5

## 14.0.0

### Major Changes

- 2a34b21ce: Rename engine and targetEngine fields to sideEffectsCacheKey.

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/types@8.3.0
  - @pnpm/fetcher-base@13.0.0
  - @pnpm/resolver-base@9.0.4

## 13.0.4

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [0abfe1718]
  - @pnpm/types@8.2.0
  - @pnpm/fetcher-base@12.1.0
  - @pnpm/resolver-base@9.0.3

## 13.0.3

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/fetcher-base@12.0.3
  - @pnpm/resolver-base@9.0.2

## 13.0.2

### Patch Changes

- 6756c2b02: It should be possible to install a git-hosted package that has no `package.json` file [#4822](https://github.com/pnpm/pnpm/issues/4822).
- Updated dependencies [6756c2b02]
  - @pnpm/fetcher-base@12.0.2

## 13.0.1

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/fetcher-base@12.0.1
  - @pnpm/resolver-base@9.0.1

## 13.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/fetcher-base@12.0.0
  - @pnpm/resolver-base@9.0.0

## 12.0.0

### Major Changes

- 5c525db13: Changes to RequestPackageOptions: currentPkg.name and currentPkg.version removed.

## 11.0.12

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/fetcher-base@11.1.6
  - @pnpm/resolver-base@8.1.6

## 11.0.11

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/fetcher-base@11.1.5
  - @pnpm/resolver-base@8.1.5

## 11.0.10

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/fetcher-base@11.1.4
  - @pnpm/resolver-base@8.1.4

## 11.0.9

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/fetcher-base@11.1.3
  - @pnpm/resolver-base@8.1.3

## 11.0.8

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/fetcher-base@11.1.2
  - @pnpm/resolver-base@8.1.2

## 11.0.7

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/fetcher-base@11.1.1
  - @pnpm/resolver-base@8.1.1

## 11.0.6

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/fetcher-base@11.1.0
  - @pnpm/resolver-base@8.1.0

## 11.0.5

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/fetcher-base@11.0.3
  - @pnpm/resolver-base@8.0.4

## 11.0.4

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/fetcher-base@11.0.2
  - @pnpm/resolver-base@8.0.3

## 11.0.3

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/fetcher-base@11.0.1
  - @pnpm/resolver-base@8.0.2

## 11.0.2

### Patch Changes

- Updated dependencies [e6a2654a2]
- Updated dependencies [e6a2654a2]
  - @pnpm/fetcher-base@11.0.0

## 11.0.1

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/resolver-base@8.0.1

## 11.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
  - @pnpm/resolver-base@8.0.0
  - @pnpm/types@7.0.0

## 10.0.0

### Major Changes

- 8d1dfa89c: Breaking changes to the store controller API.

  The options to `requestPackage()` and `fetchPackage()` changed.

## 9.2.1

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/resolver-base@7.1.1

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
