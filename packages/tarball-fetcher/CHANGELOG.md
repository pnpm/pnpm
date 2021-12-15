# @pnpm/tarball-fetcher

## 9.3.13

### Patch Changes

- Updated dependencies [eec4b195d]
  - @pnpm/prepare-package@1.0.9
  - @pnpm/core-loggers@6.1.1
  - @pnpm/fetcher-base@11.1.3

## 9.3.12

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/core-loggers@6.1.0
  - @pnpm/fetcher-base@11.1.2
  - @pnpm/prepare-package@1.0.8

## 9.3.11

### Patch Changes

- b13e4b452: Fixes a regression introduced in pnpm v6.23.3 via [#4044](https://github.com/pnpm/pnpm/pull/4044).

  The temporary directory to which the Git-hosted package is downloaded should not be removed prematurely [#4064](https://github.com/pnpm/pnpm/issues/4064).

## 9.3.10

### Patch Changes

- fb1a95a6c: The temporary directory should be removed after preparing the git-hosted package.
- fb1a95a6c: Fetch is not retried if preparation of git-hosted package fails.
- Updated dependencies [fb1a95a6c]
  - @pnpm/prepare-package@1.0.7

## 9.3.9

### Patch Changes

- @pnpm/core-loggers@6.0.6
- @pnpm/fetcher-base@11.1.1
- @pnpm/prepare-package@1.0.6

## 9.3.8

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/fetcher-base@11.1.0
  - @pnpm/core-loggers@6.0.5
  - @pnpm/prepare-package@1.0.5

## 9.3.7

### Patch Changes

- Updated dependencies [4a4d42d8f]
  - @pnpm/prepare-package@1.0.4

## 9.3.6

### Patch Changes

- Updated dependencies [bab172385]
  - @pnpm/fetching-types@2.2.1

## 9.3.5

### Patch Changes

- Updated dependencies [eadf0e505]
  - @pnpm/fetching-types@2.2.0

## 9.3.4

### Patch Changes

- @pnpm/core-loggers@6.0.4
- @pnpm/fetcher-base@11.0.3
- @pnpm/prepare-package@1.0.3

## 9.3.3

### Patch Changes

- @pnpm/core-loggers@6.0.3
- @pnpm/fetcher-base@11.0.2
- @pnpm/prepare-package@1.0.2

## 9.3.2

### Patch Changes

- @pnpm/core-loggers@6.0.2
- @pnpm/fetcher-base@11.0.1
- @pnpm/prepare-package@1.0.1

## 9.3.1

### Patch Changes

- a1a03d145: Import only the required functions from ramda.

## 9.3.0

### Minor Changes

- 6d2ccc9a3: Export waitForFilesIndex().

## 9.2.2

### Patch Changes

- Updated dependencies [a2aeeef88]
  - @pnpm/graceful-fs@1.0.0

## 9.2.1

### Patch Changes

- 3b147ced9: Do not remove the Git temporary directory because it might still be in the process of linking to the CAFS.

## 9.2.0

### Minor Changes

- e6a2654a2: Packages fetched from Git should have their `devDependencies` installed in case they have a `prepare` script.

### Patch Changes

- Updated dependencies [e6a2654a2]
- Updated dependencies [e6a2654a2]
- Updated dependencies [e6a2654a2]
  - @pnpm/prepare-package@1.0.0
  - @pnpm/fetcher-base@11.0.0

## 9.1.0

### Minor Changes

- 05baaa6e7: Add new option: timeout.

### Patch Changes

- Updated dependencies [05baaa6e7]
  - @pnpm/fetching-types@2.1.0
  - @pnpm/core-loggers@6.0.1
  - @pnpm/fetcher-base@10.0.1

## 9.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- 83645c8ed: Update ssri.
- Updated dependencies [97b986fbc]
- Updated dependencies [90487a3a8]
  - @pnpm/core-loggers@6.0.0
  - @pnpm/error@2.0.0
  - @pnpm/fetcher-base@10.0.0
  - @pnpm/fetching-types@2.0.0

## 8.2.8

### Patch Changes

- ad113645b: pin graceful-fs to v4.2.4

## 8.2.7

### Patch Changes

- @pnpm/core-loggers@5.0.3
- @pnpm/fetcher-base@9.0.4

## 8.2.6

### Patch Changes

- @pnpm/fetcher-base@9.0.3

## 8.2.5

### Patch Changes

- 0c5f1bcc9: Throw a better error message when a local tarball integrity check fails.
- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0

## 8.2.4

### Patch Changes

- @pnpm/core-loggers@5.0.2
- @pnpm/fetcher-base@9.0.2

## 8.2.3

### Patch Changes

- @pnpm/core-loggers@5.0.1
- @pnpm/fetcher-base@9.0.1

## 8.2.2

### Patch Changes

- Updated dependencies [0a6544043]
  - @pnpm/fetcher-base@9.0.0

## 8.2.1

### Patch Changes

- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0

## 8.2.0

### Minor Changes

- 7605570e6: Download progress should be logged only for big tarballs.

## 8.1.1

### Patch Changes

- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1

## 8.1.0

### Minor Changes

- 6d480dd7a: Report whether/what authorization header was used to make the request, when the request fails with an authorization issue.

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0

## 8.0.1

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0

## 8.0.0

### Major Changes

- 71aeb9a38: Breaking changes to the API. fetchFromRegistry and getCredentials are passed in through arguments.

### Patch Changes

- Updated dependencies [71aeb9a38]
  - @pnpm/fetching-types@1.0.0

## 7.1.4

### Patch Changes

- b7b026822: Pass the proxy settings to the fetcher.

## 7.1.3

### Patch Changes

- @pnpm/core-loggers@4.1.2
- @pnpm/fetcher-base@8.0.2
- fetch-from-npm-registry@4.1.2

## 7.1.2

### Patch Changes

- 1520e3d6f: Update graceful-fs to v4.2.4

## 7.1.1

### Patch Changes

- @pnpm/core-loggers@4.1.1
- @pnpm/fetcher-base@8.0.1
- fetch-from-npm-registry@4.1.1

## 7.1.0

### Minor Changes

- 2ebb7af33: Print a warning when tarball request fails.

### Patch Changes

- Updated dependencies [2ebb7af33]
- Updated dependencies [2ebb7af33]
  - fetch-from-npm-registry@4.1.0
  - @pnpm/core-loggers@4.1.0

## 7.0.1

### Patch Changes

- Updated dependencies [872f81ca1]
  - fetch-from-npm-registry@4.0.3

## 7.0.0

### Major Changes

- bcd4aa1aa: Remove `cachedTarballLocation` from `FetchOptions`. pnpm v5 doesn't store the package tarball files in the cache anymore.

### Patch Changes

- Updated dependencies [bcd4aa1aa]
  - @pnpm/fetcher-base@8.0.0

## 6.0.0

### Major Changes

- 7db36dcb3: There is no reason to keep the tarballs on the disk.
  All the files are unpacked and their checksums are stored.
  So the tarball is only used if someone modifies the content of
  the unpacked package. In that rare case, it is fine if we
  redownload the tarball from the registry.
- b6a82072e: Using a content-addressable filesystem for storing packages.

### Minor Changes

- f516d266c: Executables are saved into a separate directory inside the content-addressable storage.
- 42e6490d1: When a new package is being added to the store, its manifest is streamed in the memory. So instead of reading the manifest from the filesystem, we can parse the stream from the memory.

### Patch Changes

- c47babd52: Fix installation of local dependency from a different disk.
- f93583d52: Use `fs.mkdir` instead of the `make-dir` package.
- 1ae66a0dc: Don't create a directory for the tarball because the tarball is not saved to the filesystem anymore.
- Updated dependencies [f516d266c]
- Updated dependencies [b6a82072e]
- Updated dependencies [42e6490d1]
  - @pnpm/fetcher-base@7.0.0
  - @pnpm/error@1.2.1
  - fetch-from-npm-registry@4.0.3

## 6.0.0-alpha.4

### Patch Changes

- c47babd5: Fix installation of local dependency from a different disk.
  - @pnpm/fetcher-base@6.0.1-alpha.3

## 6.0.0-alpha.3

### Patch Changes

- 1ae66a0dc: Don't create a directory for the tarball because the tarball is not saved to the filesystem anymore.

## 6.0.0-alpha.2

### Major Changes

- 7db36dcb3: There is no reason to keep the tarballs on the disk.
  All the files are unpacked and their checksums are stored.
  So the tarball is only used if someone modifies the content of
  the unpacked package. In that rare case, it is fine if we
  redownload the tarball from the registry.

### Minor Changes

- 42e6490d1: When a new package is being added to the store, its manifest is streamed in the memory. So instead of reading the manifest from the filesystem, we can parse the stream from the memory.

### Patch Changes

- Updated dependencies [42e6490d1]
  - @pnpm/fetcher-base@7.0.0-alpha.2

## 6.0.0-alpha.1

### Minor Changes

- 4f62d0383: Executables are saved into a separate directory inside the content-addressable storage.

### Patch Changes

- f93583d52: Use `fs.mkdir` instead of the `make-dir` package.
- Updated dependencies [4f62d0383]
  - @pnpm/fetcher-base@7.0.0-alpha.1

## 6.0.0-alpha.0

### Major Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/fetcher-base@7.0.0-alpha.0

## 5.1.15

### Patch Changes

- 907c63a48: Dependencies updated.
