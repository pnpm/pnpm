# @pnpm/package-requester

## 12.2.0

### Minor Changes

- 8698a7060: New option added: preferWorkspacePackages. When it is `true`, dependencies are linked from the workspace even, when there are newer version available in the registry.

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/resolver-base@7.1.0
  - @pnpm/store-controller-types@9.2.0
  - @pnpm/fetcher-base@9.0.3
  - @pnpm/cafs@2.0.4

## 12.1.4

### Patch Changes

- @pnpm/read-package-json@3.1.8

## 12.1.3

### Patch Changes

- Updated dependencies [b3059f4f8]
  - @pnpm/cafs@2.0.3

## 12.1.2

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/core-loggers@5.0.2
  - @pnpm/fetcher-base@9.0.2
  - @pnpm/read-package-json@3.1.7
  - @pnpm/resolver-base@7.0.5
  - @pnpm/store-controller-types@9.1.2
  - @pnpm/cafs@2.0.2

## 12.1.1

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [212671848]
  - @pnpm/types@6.3.0
  - @pnpm/read-package-json@3.1.6
  - @pnpm/core-loggers@5.0.1
  - @pnpm/fetcher-base@9.0.1
  - @pnpm/resolver-base@7.0.4
  - @pnpm/store-controller-types@9.1.1
  - @pnpm/cafs@2.0.1

## 12.1.0

### Minor Changes

- 0a6544043: A new field added to the package files index: `checkedAt`. `checkedAt` is the timestamp (number of milliseconds), when the file's content was verified the last time.

### Patch Changes

- Updated dependencies [0a6544043]
- Updated dependencies [0a6544043]
- Updated dependencies [0a6544043]
  - @pnpm/store-controller-types@9.1.0
  - @pnpm/cafs@2.0.0
  - @pnpm/fetcher-base@9.0.0

## 12.0.13

### Patch Changes

- Updated dependencies [86cd72de3]
- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0
  - @pnpm/store-controller-types@9.0.0
  - @pnpm/cafs@1.0.8

## 12.0.12

### Patch Changes

- 501efdabd: The `finishing` promise is resolved always after the `files` promise.

## 12.0.11

### Patch Changes

- @pnpm/read-package-json@3.1.5

## 12.0.10

### Patch Changes

- Updated dependencies [9f5803187]
  - @pnpm/read-package-json@3.1.4

## 12.0.9

### Patch Changes

- Updated dependencies [1525fff4c]
  - @pnpm/cafs@1.0.7

## 12.0.8

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - @pnpm/cafs@1.0.6

## 12.0.7

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0

## 12.0.6

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - @pnpm/fetcher-base@8.0.2
  - @pnpm/read-package-json@3.1.3
  - @pnpm/resolver-base@7.0.3
  - @pnpm/store-controller-types@8.0.2
  - @pnpm/cafs@1.0.5

## 12.0.5

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/core-loggers@4.1.1
  - @pnpm/fetcher-base@8.0.1
  - @pnpm/read-package-json@3.1.2
  - @pnpm/resolver-base@7.0.2
  - @pnpm/store-controller-types@8.0.1
  - @pnpm/cafs@1.0.4

## 12.0.4

### Patch Changes

- Updated dependencies [492805ee3]
  - @pnpm/cafs@1.0.3

## 12.0.3

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [d3ddd023c]
- Updated dependencies [2ebb7af33]
  - @pnpm/cafs@1.0.2
  - @pnpm/core-loggers@4.1.0

## 12.0.2

### Patch Changes

- a203bc138: The temporary file name to which the package index is written should not be longer than the target file name.

## 12.0.1

### Patch Changes

- Updated dependencies [bcd4aa1aa]
  - @pnpm/fetcher-base@8.0.0
  - @pnpm/cafs@1.0.1

## 12.0.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- 9596774f2: Store the package index files in the CAFS to reduce directory nesting.
- 16d1ac0fd: `body.cacheByEngine` removed from `PackageResponse`.
- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- b6a82072e: Using a content-addressable filesystem for storing packages.
- 802d145fc: `getPackageLocation()` removed from store. Remove `inStoreLocation` from the result of `fetchPackage()`.
- 471149e66: Change the format of the package index file. Move all the files info into a "files" property.

### Minor Changes

- f516d266c: Executables are saved into a separate directory inside the content-addressable storage.
- 42e6490d1: The fetch package to store function does not need the pkgName anymore.
- a5febb913: Package request response contains the path to the files index file.
- 42e6490d1: When a new package is being added to the store, its manifest is streamed in the memory. So instead of reading the manifest from the filesystem, we can parse the stream from the memory.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- 64bae33c4: Update p-queue to v6.4.0.
- f93583d52: Use `fs.mkdir` instead of the `make-dir` package.
- c207d994f: Update rename-overwrite to v3.
- Updated dependencies [9596774f2]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [f516d266c]
- Updated dependencies [7852deea3]
- Updated dependencies [da091c711]
- Updated dependencies [42e6490d1]
- Updated dependencies [a5febb913]
- Updated dependencies [b6a82072e]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [a5febb913]
- Updated dependencies [c207d994f]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [471149e66]
- Updated dependencies [42e6490d1]
  - @pnpm/cafs@1.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/fetcher-base@7.0.0
  - @pnpm/types@6.0.0
  - @pnpm/core-loggers@4.0.2
  - @pnpm/read-package-json@3.1.1
  - @pnpm/resolver-base@7.0.1

## 12.0.0-alpha.5

### Major Changes

- 16d1ac0fd: `body.cacheByEngine` removed from `PackageResponse`.

### Minor Changes

- a5febb913: Package request response contains the path to the files index file.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [16d1ac0fd]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/cafs@1.0.0-alpha.5

## 12.0.0-alpha.4

### Major Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- 471149e6: Change the format of the package index file. Move all the files info into a "files" property.

### Patch Changes

- Updated dependencies [da091c71]
- Updated dependencies [471149e6]
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/cafs@1.0.0-alpha.4
  - @pnpm/core-loggers@4.0.2-alpha.0
  - @pnpm/fetcher-base@6.0.1-alpha.3
  - @pnpm/read-package-json@3.1.1-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0

## 12.0.0-alpha.3

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- 9596774f2: Store the package index files in the CAFS to reduce directory nesting.

### Patch Changes

- Updated dependencies [9596774f2]
- Updated dependencies [7852deea3]
  - @pnpm/cafs@1.0.0-alpha.3

## 12.0.0-alpha.2

### Minor Changes

- 42e6490d1: The fetch package to store function does not need the pkgName anymore.
- 42e6490d1: When a new package is being added to the store, its manifest is streamed in the memory. So instead of reading the manifest from the filesystem, we can parse the stream from the memory.

### Patch Changes

- 64bae33c4: Update p-queue to v6.4.0.
- c207d994f: Update rename-overwrite to v3.
- Updated dependencies [42e6490d1]
- Updated dependencies [c207d994f]
- Updated dependencies [42e6490d1]
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/cafs@1.0.0-alpha.2
  - @pnpm/fetcher-base@7.0.0-alpha.2

## 12.0.0-alpha.1

### Minor Changes

- 4f62d0383: Executables are saved into a separate directory inside the content-addressable storage.

### Patch Changes

- f93583d52: Use `fs.mkdir` instead of the `make-dir` package.
- Updated dependencies [4f62d0383]
  - @pnpm/cafs@1.0.0-alpha.1
  - @pnpm/fetcher-base@7.0.0-alpha.1
  - @pnpm/store-controller-types@8.0.0-alpha.1

## 12.0.0-alpha.0

### Major Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
- Updated dependencies [91c4b5954]
  - @pnpm/cafs@1.0.0-alpha.0
  - @pnpm/fetcher-base@7.0.0-alpha.0
  - @pnpm/store-controller-types@8.0.0-alpha.0

## 11.0.6

### Patch Changes

- 907c63a48: Update symlink-dir to v4.
- 907c63a48: Dependencies updated.
