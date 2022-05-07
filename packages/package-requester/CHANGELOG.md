# @pnpm/package-requester

## 18.0.3

### Patch Changes

- Updated dependencies [cadefe5b6]
  - @pnpm/cafs@4.0.2

## 18.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/core-loggers@7.0.1
  - dependency-path@9.1.1
  - @pnpm/fetcher-base@12.0.1
  - @pnpm/package-is-installable@6.0.2
  - @pnpm/read-package-json@6.0.2
  - @pnpm/resolver-base@9.0.1
  - @pnpm/store-controller-types@13.0.1
  - @pnpm/cafs@4.0.1

## 18.0.1

### Patch Changes

- 7cdca5ef2: Don't check the integrity of the store with the package version from the lockfile, when the package was updated [#4580](https://github.com/pnpm/pnpm/pull/4580).
- Updated dependencies [0a70aedb1]
  - dependency-path@9.1.0
  - @pnpm/error@3.0.1
  - @pnpm/package-is-installable@6.0.1
  - @pnpm/read-package-json@6.0.1

## 18.0.0

### Major Changes

- 9c22c063e: Local dependencies referenced through the `file:` protocol are hard linked (not symlinked) [#4408](https://github.com/pnpm/pnpm/pull/4408). If you need to symlink a dependency, use the `link:` protocol instead.
- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [faf830b8f]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - dependency-path@9.0.0
  - @pnpm/cafs@4.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/error@3.0.0
  - @pnpm/fetcher-base@12.0.0
  - @pnpm/graceful-fs@2.0.0
  - @pnpm/package-is-installable@6.0.0
  - @pnpm/read-package-json@6.0.0
  - @pnpm/resolver-base@9.0.0
  - @pnpm/store-controller-types@13.0.0

## 17.0.0

### Major Changes

- 5c525db13: Changes to RequestPackageOptions: currentPkg.name and currentPkg.version removed.

### Patch Changes

- Updated dependencies [70ba51da9]
- Updated dependencies [5c525db13]
  - @pnpm/error@2.1.0
  - @pnpm/store-controller-types@12.0.0
  - @pnpm/package-is-installable@5.0.13
  - @pnpm/read-package-json@5.0.12
  - @pnpm/cafs@3.0.15

## 16.0.2

### Patch Changes

- 800fb2836: Ignore case, when verifying package name in the store [#4367](https://github.com/pnpm/pnpm/issues/4367).
- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/core-loggers@6.1.4
  - dependency-path@8.0.11
  - @pnpm/fetcher-base@11.1.6
  - @pnpm/package-is-installable@5.0.12
  - @pnpm/read-package-json@5.0.11
  - @pnpm/resolver-base@8.1.6
  - @pnpm/store-controller-types@11.0.12
  - @pnpm/cafs@3.0.14

## 16.0.1

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/core-loggers@6.1.3
  - dependency-path@8.0.10
  - @pnpm/fetcher-base@11.1.5
  - @pnpm/package-is-installable@5.0.11
  - @pnpm/read-package-json@5.0.10
  - @pnpm/resolver-base@8.1.5
  - @pnpm/store-controller-types@11.0.11
  - @pnpm/cafs@3.0.13

## 16.0.0

### Major Changes

- 8ddcd5116: Log the fetch statuses of packages for the progress reporter.

## 15.2.6

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/core-loggers@6.1.2
  - dependency-path@8.0.9
  - @pnpm/fetcher-base@11.1.4
  - @pnpm/package-is-installable@5.0.10
  - @pnpm/read-package-json@5.0.9
  - @pnpm/resolver-base@8.1.4
  - @pnpm/store-controller-types@11.0.10
  - @pnpm/cafs@3.0.12

## 15.2.5

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/core-loggers@6.1.1
  - dependency-path@8.0.8
  - @pnpm/fetcher-base@11.1.3
  - @pnpm/package-is-installable@5.0.9
  - @pnpm/read-package-json@5.0.8
  - @pnpm/resolver-base@8.1.3
  - @pnpm/store-controller-types@11.0.9
  - @pnpm/cafs@3.0.11

## 15.2.4

### Patch Changes

- 77ff0898b: Don't fail when the version of a package in the store is not a semver version.
- Updated dependencies [ba9b2eba1]
- Updated dependencies [ba9b2eba1]
  - @pnpm/core-loggers@6.1.0
  - @pnpm/types@7.7.0
  - @pnpm/package-is-installable@5.0.8
  - dependency-path@8.0.7
  - @pnpm/fetcher-base@11.1.2
  - @pnpm/read-package-json@5.0.7
  - @pnpm/resolver-base@8.1.2
  - @pnpm/store-controller-types@11.0.8
  - @pnpm/cafs@3.0.10

## 15.2.3

### Patch Changes

- dbd8acfe9: The version in the bundled manifest should always be normalized.
- 119b3a908: When checking the correctness of the package data in the lockfile, don't use exact version comparison. `v1.0.0` should be considered to be the same as `1.0.0`. This fixes some edge cases when a package is published with a non-normalized version specifier in its `package.json`.

## 15.2.2

### Patch Changes

- Updated dependencies [783cc1051]
  - @pnpm/package-is-installable@5.0.7

## 15.2.1

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/core-loggers@6.0.6
  - dependency-path@8.0.6
  - @pnpm/fetcher-base@11.1.1
  - @pnpm/package-is-installable@5.0.6
  - @pnpm/read-package-json@5.0.6
  - @pnpm/resolver-base@8.1.1
  - @pnpm/store-controller-types@11.0.7
  - @pnpm/cafs@3.0.9

## 15.2.0

### Minor Changes

- 4ab87844a: Added support for "injected" dependencies.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/fetcher-base@11.1.0
  - @pnpm/resolver-base@8.1.0
  - @pnpm/core-loggers@6.0.5
  - dependency-path@8.0.5
  - @pnpm/package-is-installable@5.0.5
  - @pnpm/read-package-json@5.0.5
  - @pnpm/store-controller-types@11.0.6
  - @pnpm/cafs@3.0.8

## 15.1.2

### Patch Changes

- 11a934da1: Always fetch the bundled manifest.

## 15.1.1

### Patch Changes

- 31e01d9a9: isInstallable should be always returned by `packageRequester()`.

## 15.1.0

### Minor Changes

- 07e7b1c0c: Do not fetch optional packages that are not installable on the target system.

## 15.0.7

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/core-loggers@6.0.4
  - dependency-path@8.0.4
  - @pnpm/fetcher-base@11.0.3
  - @pnpm/read-package-json@5.0.4
  - @pnpm/resolver-base@8.0.4
  - @pnpm/store-controller-types@11.0.5
  - @pnpm/cafs@3.0.7

## 15.0.6

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/core-loggers@6.0.3
  - dependency-path@8.0.3
  - @pnpm/fetcher-base@11.0.2
  - @pnpm/read-package-json@5.0.3
  - @pnpm/resolver-base@8.0.3
  - @pnpm/store-controller-types@11.0.4
  - @pnpm/cafs@3.0.6

## 15.0.5

### Patch Changes

- Updated dependencies [6c418943c]
  - dependency-path@8.0.2

## 15.0.4

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/core-loggers@6.0.2
  - dependency-path@8.0.1
  - @pnpm/fetcher-base@11.0.1
  - @pnpm/read-package-json@5.0.2
  - @pnpm/resolver-base@8.0.2
  - @pnpm/store-controller-types@11.0.3
  - @pnpm/cafs@3.0.5

## 15.0.3

### Patch Changes

- a1a03d145: Import only the required functions from ramda.

## 15.0.2

### Patch Changes

- Updated dependencies [20e2f235d]
  - dependency-path@8.0.0

## 15.0.1

### Patch Changes

- Updated dependencies [a2aeeef88]
- Updated dependencies [ef0ca24be]
  - @pnpm/graceful-fs@1.0.0
  - @pnpm/cafs@3.0.4

## 15.0.0

### Major Changes

- e6a2654a2: Breaking changes to the API of `packageRequester()`.

  `resolve` and `fetchers` should be passed in through `options`, not as arguments.

  `cafs` is not returned anymore. It should be passed in through `options` as well.

### Patch Changes

- Updated dependencies [e6a2654a2]
- Updated dependencies [e6a2654a2]
  - @pnpm/fetcher-base@11.0.0
  - @pnpm/cafs@3.0.3
  - @pnpm/store-controller-types@11.0.2

## 14.0.3

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/core-loggers@6.0.1
  - dependency-path@7.0.1
  - @pnpm/fetcher-base@10.0.1
  - @pnpm/read-package-json@5.0.1
  - @pnpm/resolver-base@8.0.1
  - @pnpm/store-controller-types@11.0.1
  - @pnpm/cafs@3.0.2

## 14.0.2

### Patch Changes

- 6f198457d: Update rename-overwrite.
- Updated dependencies [6f198457d]
  - @pnpm/cafs@3.0.1

## 14.0.1

### Patch Changes

- Updated dependencies [9ceab68f0]
  - dependency-path@7.0.0

## 14.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- 83645c8ed: Update ssri.
- Updated dependencies [97b986fbc]
- Updated dependencies [90487a3a8]
- Updated dependencies [e4efddbd2]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [83645c8ed]
  - @pnpm/cafs@3.0.0
  - @pnpm/core-loggers@6.0.0
  - dependency-path@6.0.0
  - @pnpm/error@2.0.0
  - @pnpm/fetcher-base@10.0.0
  - @pnpm/read-package-json@5.0.0
  - @pnpm/resolver-base@8.0.0
  - @pnpm/store-controller-types@11.0.0
  - @pnpm/types@7.0.0

## 13.0.1

### Patch Changes

- Updated dependencies [d853fb14a]
  - @pnpm/read-package-json@4.0.0

## 13.0.0

### Major Changes

- 8d1dfa89c: Breaking changes to the store controller API.

  The options to `requestPackage()` and `fetchPackage()` changed.

### Patch Changes

- Updated dependencies [8d1dfa89c]
- Updated dependencies [8d1dfa89c]
  - @pnpm/store-controller-types@10.0.0
  - @pnpm/cafs@2.1.0

## 12.2.2

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/core-loggers@5.0.3
  - dependency-path@5.1.1
  - @pnpm/fetcher-base@9.0.4
  - @pnpm/read-package-json@3.1.9
  - @pnpm/resolver-base@7.1.1
  - @pnpm/store-controller-types@9.2.1
  - @pnpm/cafs@2.0.5

## 12.2.1

### Patch Changes

- Updated dependencies [e27dcf0dc]
  - dependency-path@5.1.0

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
