# @pnpm/package-store

## 10.1.12

### Patch Changes

- @pnpm/package-requester@12.2.0

## 10.1.11

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/package-requester@12.2.0
  - @pnpm/resolver-base@7.1.0
  - @pnpm/store-controller-types@9.2.0
  - @pnpm/fetcher-base@9.0.3
  - @pnpm/cafs@2.0.4

## 10.1.10

### Patch Changes

- @pnpm/package-requester@12.1.4

## 10.1.9

### Patch Changes

- @pnpm/package-requester@12.1.4

## 10.1.8

### Patch Changes

- 09492b7b4: Update write-file-atomic to v3.
  - @pnpm/package-requester@12.1.3

## 10.1.7

### Patch Changes

- @pnpm/package-requester@12.1.3

## 10.1.6

### Patch Changes

- 01aecf038: Do not try to copy a file during linking, if the target already exists.
- Updated dependencies [b3059f4f8]
  - @pnpm/cafs@2.0.3
  - @pnpm/package-requester@12.1.3

## 10.1.5

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/core-loggers@5.0.2
  - @pnpm/fetcher-base@9.0.2
  - @pnpm/package-requester@12.1.2
  - @pnpm/resolver-base@7.0.5
  - @pnpm/store-controller-types@9.1.2
  - @pnpm/cafs@2.0.2

## 10.1.4

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/core-loggers@5.0.1
  - @pnpm/fetcher-base@9.0.1
  - @pnpm/package-requester@12.1.1
  - @pnpm/resolver-base@7.0.4
  - @pnpm/store-controller-types@9.1.1
  - @pnpm/cafs@2.0.1

## 10.1.3

### Patch Changes

- @pnpm/package-requester@12.1.0

## 10.1.2

### Patch Changes

- @pnpm/package-requester@12.1.0

## 10.1.1

### Patch Changes

- @pnpm/package-requester@12.1.0

## 10.1.0

### Minor Changes

- 0a6544043: A new field added to the package files index: `checkedAt`. `checkedAt` is the timestamp (number of milliseconds), when the file's content was verified the last time.

### Patch Changes

- Updated dependencies [0a6544043]
- Updated dependencies [0a6544043]
- Updated dependencies [0a6544043]
  - @pnpm/package-requester@12.1.0
  - @pnpm/store-controller-types@9.1.0
  - @pnpm/cafs@2.0.0
  - @pnpm/fetcher-base@9.0.0

## 10.0.2

### Patch Changes

- d94b19b39: Unless an EXDEV error is thrown during hard linking, always choose hard linking for importing packages from the store.

## 10.0.1

### Patch Changes

- 7f74cd173: Fixing a regression. Package should be imported when import method is being identified.

## 10.0.0

### Major Changes

- 86cd72de3: The `importPackage` function of the store controller returns the `importMethod` that was used to link the package to the virtual store. If importing was not needed, `importMethod` is `undefined`.

### Patch Changes

- Updated dependencies [86cd72de3]
- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0
  - @pnpm/store-controller-types@9.0.0
  - @pnpm/package-requester@12.0.13
  - @pnpm/cafs@1.0.8

## 9.1.8

### Patch Changes

- 6457562c4: When `package-import-method` is set to `auto`, cloning is only tried once. If it fails, it is not retried for other packages.
- 6457562c4: Report package importing once it actually succeeds.
  - @pnpm/package-requester@12.0.12

## 9.1.7

### Patch Changes

- Updated dependencies [501efdabd]
  - @pnpm/package-requester@12.0.12

## 9.1.6

### Patch Changes

- @pnpm/package-requester@12.0.11

## 9.1.5

### Patch Changes

- @pnpm/package-requester@12.0.10

## 9.1.4

### Patch Changes

- Updated dependencies [1525fff4c]
  - @pnpm/cafs@1.0.7
  - @pnpm/package-requester@12.0.9

## 9.1.3

### Patch Changes

- @pnpm/package-requester@12.0.8

## 9.1.2

### Patch Changes

- @pnpm/package-requester@12.0.8

## 9.1.1

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - @pnpm/cafs@1.0.6
  - @pnpm/package-requester@12.0.8

## 9.1.0

### Minor Changes

- 9a908bc07: Add packageImportMethod logger.

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0
  - @pnpm/package-requester@12.0.7

## 9.0.14

### Patch Changes

- @pnpm/package-requester@12.0.6

## 9.0.13

### Patch Changes

- @pnpm/package-requester@12.0.6

## 9.0.12

### Patch Changes

- @pnpm/package-requester@12.0.6

## 9.0.11

### Patch Changes

- @pnpm/package-requester@12.0.6

## 9.0.10

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - @pnpm/fetcher-base@8.0.2
  - @pnpm/package-requester@12.0.6
  - @pnpm/resolver-base@7.0.3
  - @pnpm/store-controller-types@8.0.2
  - @pnpm/cafs@1.0.5

## 9.0.9

### Patch Changes

- 1adacd41e: only scan diretories when doing store prune

## 9.0.8

### Patch Changes

- @pnpm/package-requester@12.0.5

## 9.0.7

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/core-loggers@4.1.1
  - @pnpm/fetcher-base@8.0.1
  - @pnpm/package-requester@12.0.5
  - @pnpm/resolver-base@7.0.2
  - @pnpm/store-controller-types@8.0.1
  - @pnpm/cafs@1.0.4

## 9.0.6

### Patch Changes

- Updated dependencies [492805ee3]
  - @pnpm/cafs@1.0.3
  - @pnpm/package-requester@12.0.4

## 9.0.5

### Patch Changes

- @pnpm/package-requester@12.0.3

## 9.0.4

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [d3ddd023c]
- Updated dependencies [2ebb7af33]
  - @pnpm/cafs@1.0.2
  - @pnpm/package-requester@12.0.3
  - @pnpm/core-loggers@4.1.0

## 9.0.3

### Patch Changes

- Updated dependencies [a203bc138]
  - @pnpm/package-requester@12.0.2

## 9.0.2

### Patch Changes

- @pnpm/package-requester@12.0.1

## 9.0.1

### Patch Changes

- 429c5a560: If creating a hard-link to a file from the store fails, fall back to copying the file.
- Updated dependencies [bcd4aa1aa]
  - @pnpm/fetcher-base@8.0.0
  - @pnpm/package-requester@12.0.1
  - @pnpm/cafs@1.0.1

## 9.0.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- b6a82072e: Using a content-addressable filesystem for storing packages.
- 802d145fc: `getPackageLocation()` removed from store. Remove `inStoreLocation` from the result of `fetchPackage()`.
- a5febb913: The importPackage function of the store controller is importing packages directly from the side-effects cache.
- a5febb913: The upload function of the store controller accepts `opts.filesIndexFile` instead of `opts.packageId`.

### Minor Changes

- cbc2192f1: Don't try to create the dependency directory twice.
- f516d266c: Executables are saved into a separate directory inside the content-addressable storage.
- ecf2c6b7d: Prune unreferenced files from the store.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- c207d994f: Update rename-overwrite to v3.
- Updated dependencies [b5f66c0f2]
- Updated dependencies [9596774f2]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [f516d266c]
- Updated dependencies [7852deea3]
- Updated dependencies [da091c711]
- Updated dependencies [a7d20d927]
- Updated dependencies [42e6490d1]
- Updated dependencies [64bae33c4]
- Updated dependencies [a5febb913]
- Updated dependencies [b6a82072e]
- Updated dependencies [f93583d52]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [a5febb913]
- Updated dependencies [c207d994f]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [471149e66]
- Updated dependencies [42e6490d1]
  - @pnpm/package-requester@12.0.0
  - @pnpm/cafs@1.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/fetcher-base@7.0.0
  - @pnpm/types@6.0.0
  - @pnpm/core-loggers@4.0.2
  - @pnpm/resolver-base@7.0.1

## 9.0.0-alpha.5

### Major Changes

- a5febb913: The importPackage function of the store controller is importing packages directly from the side-effects cache.
- a5febb913: The upload function of the store controller accepts `opts.filesIndexFile` instead of `opts.packageId`.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [16d1ac0fd]
- Updated dependencies [a7d20d927]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/package-requester@12.0.0-alpha.5
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/cafs@1.0.0-alpha.5

## 9.0.0-alpha.4

### Major Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.

### Minor Changes

- ecf2c6b7: Prune unreferenced files from the store.

### Patch Changes

- Updated dependencies [da091c71]
- Updated dependencies [471149e6]
  - @pnpm/package-requester@12.0.0-alpha.4
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/cafs@1.0.0-alpha.4
  - @pnpm/core-loggers@4.0.2-alpha.0
  - @pnpm/fetcher-base@6.0.1-alpha.3
  - @pnpm/resolver-base@7.0.1-alpha.0

## 9.0.0-alpha.3

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [9596774f2]
- Updated dependencies [7852deea3]
  - @pnpm/package-requester@12.0.0-alpha.3
  - @pnpm/cafs@1.0.0-alpha.3

## 9.0.0-alpha.2

### Patch Changes

- c207d994f: Update rename-overwrite to v3.
- Updated dependencies [42e6490d1]
- Updated dependencies [64bae33c4]
- Updated dependencies [c207d994f]
- Updated dependencies [42e6490d1]
  - @pnpm/package-requester@12.0.0-alpha.2
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/cafs@1.0.0-alpha.2
  - @pnpm/fetcher-base@7.0.0-alpha.2

## 9.0.0-alpha.1

### Minor Changes

- 4f62d0383: Executables are saved into a separate directory inside the content-addressable storage.

### Patch Changes

- Updated dependencies [4f62d0383]
- Updated dependencies [f93583d52]
  - @pnpm/cafs@1.0.0-alpha.1
  - @pnpm/fetcher-base@7.0.0-alpha.1
  - @pnpm/package-requester@12.0.0-alpha.1
  - @pnpm/store-controller-types@8.0.0-alpha.1

## 9.0.0-alpha.0

### Major Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
- Updated dependencies [91c4b5954]
  - @pnpm/cafs@1.0.0-alpha.0
  - @pnpm/fetcher-base@7.0.0-alpha.0
  - @pnpm/package-requester@12.0.0-alpha.0
  - @pnpm/store-controller-types@8.0.0-alpha.0

## 8.1.0

### Minor Changes

- 907c63a48: The number of filesystem operations has been reduced.

### Patch Changes

- 907c63a48: Dependencies updated.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/package-requester@11.0.6
