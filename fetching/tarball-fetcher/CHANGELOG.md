# @pnpm/tarball-fetcher

## 19.0.7

### Patch Changes

- @pnpm/fetcher-base@16.0.3
- @pnpm/prepare-package@6.0.7
- @pnpm/core-loggers@10.0.3
- @pnpm/worker@1.0.5

## 19.0.6

### Patch Changes

- @pnpm/prepare-package@6.0.6
- @pnpm/fetcher-base@16.0.2
- @pnpm/core-loggers@10.0.2
- @pnpm/worker@1.0.4

## 19.0.5

### Patch Changes

- @pnpm/prepare-package@6.0.5

## 19.0.4

### Patch Changes

- @pnpm/prepare-package@6.0.4
- @pnpm/worker@1.0.3

## 19.0.3

### Patch Changes

- @pnpm/prepare-package@6.0.3
- @pnpm/fetcher-base@16.0.1
- @pnpm/core-loggers@10.0.1
- @pnpm/worker@1.0.2

## 19.0.2

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/prepare-package@6.0.2
  - @pnpm/worker@1.0.1

## 19.0.1

### Patch Changes

- @pnpm/prepare-package@6.0.1

## 19.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- b13d2dc: It is now possible to install only a subdirectory from a Git repository.

  For example, `pnpm add github:user/repo#path:packages/foo` will add a dependency from the `packages/foo` subdirectory.

  This new parameter may be combined with other supported parameters separated by `&`. For instance, the next command will install the same package from the `dev` branch: `pnpm add github:user/repo#dev&path:packages/bar`.

  Related issue: [#4765](https://github.com/pnpm/pnpm/issues/4765).
  Related PR: [#7487](https://github.com/pnpm/pnpm/pull/7487).

- 730929e: Add a field named `ignoredOptionalDependencies`. This is an array of strings. If an optional dependency has its name included in this array, it will be skipped.

### Patch Changes

- 3ded840: Print the right error code when a package fails to be added to the store [#7679](https://github.com/pnpm/pnpm/issues/7679).
- 36dcaa0: When installing git-hosted dependencies, only pick the files that would be packed with the package [#7638](https://github.com/pnpm/pnpm/pull/7638).
- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
- Updated dependencies [167ac4d]
- Updated dependencies [11d9ebd]
- Updated dependencies [36dcaa0]
- Updated dependencies [b13d2dc]
- Updated dependencies [730929e]
  - @pnpm/error@6.0.0
  - @pnpm/worker@1.0.0
  - @pnpm/fetching-types@6.0.0
  - @pnpm/fetcher-base@16.0.0
  - @pnpm/core-loggers@10.0.0
  - @pnpm/prepare-package@6.0.0
  - @pnpm/graceful-fs@4.0.0
  - @pnpm/fs.packlist@2.0.0

## 18.0.19

### Patch Changes

- @pnpm/fetcher-base@15.0.7
- @pnpm/prepare-package@5.0.24
- @pnpm/worker@0.3.14

## 18.0.18

### Patch Changes

- 342222d20: A git-hosted dependency should not be added to the store if it failed to be built [#7407](https://github.com/pnpm/pnpm/pull/7407).
  - @pnpm/prepare-package@5.0.23

## 18.0.17

### Patch Changes

- @pnpm/worker@0.3.13

## 18.0.16

### Patch Changes

- @pnpm/worker@0.3.12
- @pnpm/prepare-package@5.0.22
- @pnpm/fetcher-base@15.0.6
- @pnpm/core-loggers@9.0.6

## 18.0.15

### Patch Changes

- @pnpm/prepare-package@5.0.21
- @pnpm/fetcher-base@15.0.5
- @pnpm/core-loggers@9.0.5
- @pnpm/worker@0.3.11

## 18.0.14

### Patch Changes

- @pnpm/worker@0.3.10

## 18.0.13

### Patch Changes

- Updated dependencies [1e7bd4af3]
  - @pnpm/worker@0.3.9

## 18.0.12

### Patch Changes

- abdf1f2b6: Don't retry fetching missing packages, since the retries will never work [#7276](https://github.com/pnpm/pnpm/pull/7276).
  - @pnpm/prepare-package@5.0.20
  - @pnpm/worker@0.3.8

## 18.0.11

### Patch Changes

- @pnpm/prepare-package@5.0.19

## 18.0.10

### Patch Changes

- @pnpm/fetcher-base@15.0.4
- @pnpm/worker@0.3.7
- @pnpm/prepare-package@5.0.18

## 18.0.9

### Patch Changes

- @pnpm/prepare-package@5.0.17

## 18.0.8

### Patch Changes

- Updated dependencies [6390033cd]
  - @pnpm/worker@0.3.6
  - @pnpm/prepare-package@5.0.16
  - @pnpm/fetcher-base@15.0.3
  - @pnpm/core-loggers@9.0.4

## 18.0.7

### Patch Changes

- @pnpm/prepare-package@5.0.15

## 18.0.6

### Patch Changes

- @pnpm/prepare-package@5.0.14
- @pnpm/worker@0.3.5

## 18.0.5

### Patch Changes

- Updated dependencies [08b65ff78]
  - @pnpm/worker@0.3.4
  - @pnpm/prepare-package@5.0.13

## 18.0.4

### Patch Changes

- @pnpm/worker@0.3.3

## 18.0.3

### Patch Changes

- @pnpm/worker@0.3.2

## 18.0.2

### Patch Changes

- @pnpm/prepare-package@5.0.13
- @pnpm/fetcher-base@15.0.2
- @pnpm/core-loggers@9.0.3
- @pnpm/worker@0.3.1

## 18.0.1

### Patch Changes

- Updated dependencies [17d2ddb05]
  - @pnpm/prepare-package@5.0.12

## 18.0.0

### Major Changes

- 9caa33d53: `fromStore` replaced with `resolvedFrom`.

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/worker@0.3.0
  - @pnpm/graceful-fs@3.2.0
  - @pnpm/fetcher-base@15.0.1
  - @pnpm/prepare-package@5.0.11

## 17.0.1

### Patch Changes

- @pnpm/worker@0.2.1

## 17.0.0

### Patch Changes

- Updated dependencies [03cdccc6e]
- Updated dependencies [48dcd108c]
  - @pnpm/worker@0.2.0
  - @pnpm/fetcher-base@15.0.1
  - @pnpm/prepare-package@5.0.10

## 16.0.2

### Patch Changes

- @pnpm/worker@0.1.2
- @pnpm/prepare-package@5.0.9

## 16.0.1

### Patch Changes

- Updated dependencies [4a1a9431d]
  - @pnpm/fetcher-base@15.0.1
  - @pnpm/worker@0.1.1
  - @pnpm/prepare-package@5.0.9

## 16.0.0

### Major Changes

- 083bbf590: Breaking changes to the API.
- 70b2830ac: Breaking changes to the API.

### Patch Changes

- 96e165c7f: Performance optimizations. Package tarballs are now download directly to memory and built to an ArrayBuffer. Hashing and other operations are avoided until the stream has been fully received [#6819](https://github.com/pnpm/pnpm/pull/6819).
- Updated dependencies [70b2830ac]
- Updated dependencies [083bbf590]
- Updated dependencies [083bbf590]
- Updated dependencies [083bbf590]
  - @pnpm/fetcher-base@15.0.0
  - @pnpm/worker@0.1.0
  - @pnpm/graceful-fs@3.1.0
  - @pnpm/prepare-package@5.0.8

## 15.0.9

### Patch Changes

- 840b65bda: Report download progress less often to improve performance.

## 15.0.8

### Patch Changes

- @pnpm/prepare-package@5.0.7

## 15.0.7

### Patch Changes

- Updated dependencies [8452bb2d5]
  - @pnpm/prepare-package@5.0.6

## 15.0.6

### Patch Changes

- @pnpm/prepare-package@5.0.5
- @pnpm/fetcher-base@14.0.2
- @pnpm/core-loggers@9.0.2

## 15.0.5

### Patch Changes

- @pnpm/prepare-package@5.0.4

## 15.0.4

### Patch Changes

- @pnpm/prepare-package@5.0.3

## 15.0.3

### Patch Changes

- @pnpm/error@5.0.2
- @pnpm/prepare-package@5.0.2

## 15.0.2

### Patch Changes

- d55b41a8b: Dependencies have been updated.
  - @pnpm/prepare-package@5.0.1

## 15.0.1

### Patch Changes

- @pnpm/prepare-package@5.0.1
- @pnpm/fetcher-base@14.0.1
- @pnpm/core-loggers@9.0.1
- @pnpm/error@5.0.1

## 15.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/fetching-types@5.0.0
  - @pnpm/fetcher-base@14.0.0
  - @pnpm/core-loggers@9.0.0
  - @pnpm/prepare-package@5.0.0
  - @pnpm/graceful-fs@3.0.0
  - @pnpm/error@5.0.0

## 14.1.4

### Patch Changes

- Updated dependencies [955874422]
  - @pnpm/graceful-fs@2.1.0
  - @pnpm/prepare-package@4.1.2

## 14.1.3

### Patch Changes

- 2241f77ad: Print a hint that suggests to run `pnpm store prune`, when a tarball integrity error happens.

## 14.1.2

### Patch Changes

- @pnpm/fetcher-base@13.1.6
- @pnpm/prepare-package@4.1.1

## 14.1.1

### Patch Changes

- 1e6de89b6: Update ssri to v10.0.1.
  - @pnpm/prepare-package@4.1.0

## 14.1.0

### Minor Changes

- c7b05cd9a: When ignoreScripts=true is passed to the fetcher, do not build git-hosted dependencies.

### Patch Changes

- Updated dependencies [c7b05cd9a]
  - @pnpm/prepare-package@4.1.0
  - @pnpm/error@4.0.1

## 14.0.1

### Patch Changes

- ec97a3105: Print more contextual information when a git-hosted package fails to be prepared for installation [#5847](https://github.com/pnpm/pnpm/pull/5847).
- Updated dependencies [ec97a3105]
- Updated dependencies [40a481840]
  - @pnpm/prepare-package@4.0.1

## 14.0.0

### Major Changes

- 339c0a704: A new required option added to the prepare package function: rawConfig. It is needed in order to create a proper environment for the package manager executed during the preparation of a git-hosted dependency.

### Patch Changes

- Updated dependencies [339c0a704]
- Updated dependencies [339c0a704]
  - @pnpm/prepare-package@4.0.0

## 13.0.3

### Patch Changes

- @pnpm/fetcher-base@13.1.5
- @pnpm/core-loggers@8.0.3
- @pnpm/prepare-package@3.0.4

## 13.0.2

### Patch Changes

- a9d59d8bc: Update dependencies.
  - @pnpm/prepare-package@3.0.3

## 13.0.1

### Patch Changes

- @pnpm/core-loggers@8.0.2
- @pnpm/fetcher-base@13.1.4
- @pnpm/prepare-package@3.0.2

## 13.0.0

### Major Changes

- 804de211e: GetCredentials function replaced with GetAuthHeader.

### Patch Changes

- Updated dependencies [804de211e]
  - @pnpm/fetching-types@4.0.0

## 12.0.1

### Patch Changes

- @pnpm/core-loggers@8.0.1
- @pnpm/fetcher-base@13.1.3
- @pnpm/prepare-package@3.0.1

## 12.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/core-loggers@8.0.0
  - @pnpm/prepare-package@3.0.0

## 11.0.5

### Patch Changes

- Updated dependencies [3ae888c28]
  - @pnpm/core-loggers@7.1.0

## 11.0.4

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/prepare-package@2.0.11

## 11.0.3

### Patch Changes

- @pnpm/core-loggers@7.0.8
- @pnpm/fetcher-base@13.1.2
- @pnpm/prepare-package@2.0.10

## 11.0.2

### Patch Changes

- @pnpm/core-loggers@7.0.7
- @pnpm/fetcher-base@13.1.1
- @pnpm/prepare-package@2.0.9

## 11.0.1

### Patch Changes

- dbac0ca01: Update ssri to v9.
  - @pnpm/prepare-package@2.0.8

## 11.0.0

### Major Changes

- 7a17f99ab: Refactor `tarball-fetcher` and separate it into more specific fetchers, such as `localTarball`, `remoteTarball` and `gitHostedTarball`.

### Patch Changes

- 32915f0e4: Refactor cafs types into separate package and add additional properties including `cafsDir` and `getFilePathInCafs`.
- Updated dependencies [32915f0e4]
- Updated dependencies [23984abd1]
  - @pnpm/fetcher-base@13.1.0

## 10.0.10

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270

## 10.0.9

### Patch Changes

- @pnpm/core-loggers@7.0.6
- @pnpm/fetcher-base@13.0.2
- @pnpm/prepare-package@2.0.7

## 10.0.8

### Patch Changes

- 5f643f23b: Update ramda to v0.28.

## 10.0.7

### Patch Changes

- @pnpm/core-loggers@7.0.5
- @pnpm/fetcher-base@13.0.1
- @pnpm/prepare-package@2.0.6

## 10.0.6

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/fetcher-base@13.0.0
  - @pnpm/core-loggers@7.0.4
  - @pnpm/prepare-package@2.0.5

## 10.0.5

### Patch Changes

- Updated dependencies [0abfe1718]
  - @pnpm/fetcher-base@12.1.0
  - @pnpm/core-loggers@7.0.3
  - @pnpm/prepare-package@2.0.4

## 10.0.4

### Patch Changes

- @pnpm/core-loggers@7.0.2
- @pnpm/fetcher-base@12.0.3
- @pnpm/prepare-package@2.0.3

## 10.0.3

### Patch Changes

- Updated dependencies [6756c2b02]
  - @pnpm/fetcher-base@12.0.2

## 10.0.2

### Patch Changes

- @pnpm/core-loggers@7.0.1
- @pnpm/fetcher-base@12.0.1
- @pnpm/prepare-package@2.0.2

## 10.0.1

### Patch Changes

- @pnpm/error@3.0.1
- @pnpm/prepare-package@2.0.1

## 10.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/core-loggers@7.0.0
  - @pnpm/error@3.0.0
  - @pnpm/fetcher-base@12.0.0
  - @pnpm/fetching-types@3.0.0
  - @pnpm/graceful-fs@2.0.0
  - @pnpm/prepare-package@2.0.0

## 9.3.17

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/prepare-package@1.0.13

## 9.3.16

### Patch Changes

- @pnpm/core-loggers@6.1.4
- @pnpm/fetcher-base@11.1.6
- @pnpm/prepare-package@1.0.12

## 9.3.15

### Patch Changes

- @pnpm/core-loggers@6.1.3
- @pnpm/fetcher-base@11.1.5
- @pnpm/prepare-package@1.0.11

## 9.3.14

### Patch Changes

- @pnpm/core-loggers@6.1.2
- @pnpm/fetcher-base@11.1.4
- @pnpm/prepare-package@1.0.10

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
