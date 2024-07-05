# @pnpm/git-fetcher

## 13.0.7

### Patch Changes

- @pnpm/fetcher-base@16.0.3
- @pnpm/prepare-package@6.0.7
- @pnpm/worker@1.0.5

## 13.0.6

### Patch Changes

- @pnpm/prepare-package@6.0.6
- @pnpm/fetcher-base@16.0.2
- @pnpm/worker@1.0.4

## 13.0.5

### Patch Changes

- @pnpm/prepare-package@6.0.5

## 13.0.4

### Patch Changes

- @pnpm/prepare-package@6.0.4
- @pnpm/worker@1.0.3

## 13.0.3

### Patch Changes

- @pnpm/prepare-package@6.0.3
- @pnpm/fetcher-base@16.0.1
- @pnpm/worker@1.0.2

## 13.0.2

### Patch Changes

- @pnpm/prepare-package@6.0.2
- @pnpm/worker@1.0.1

## 13.0.1

### Patch Changes

- @pnpm/prepare-package@6.0.1

## 13.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- b13d2dc: It is now possible to install only a subdirectory from a Git repository.

  For example, `pnpm add github:user/repo#path:packages/foo` will add a dependency from the `packages/foo` subdirectory.

  This new parameter may be combined with other supported parameters separated by `&`. For instance, the next command will install the same package from the `dev` branch: `pnpm add github:user/repo#dev&path:packages/bar`.

  Related issue: [#4765](https://github.com/pnpm/pnpm/issues/4765).
  Related PR: [#7487](https://github.com/pnpm/pnpm/pull/7487).

### Patch Changes

- 36dcaa0: When installing git-hosted dependencies, only pick the files that would be packed with the package [#7638](https://github.com/pnpm/pnpm/pull/7638).
- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
- Updated dependencies [167ac4d]
- Updated dependencies [11d9ebd]
- Updated dependencies [36dcaa0]
- Updated dependencies [b13d2dc]
- Updated dependencies [730929e]
  - @pnpm/worker@1.0.0
  - @pnpm/fetcher-base@16.0.0
  - @pnpm/prepare-package@6.0.0
  - @pnpm/fs.packlist@2.0.0

## 12.0.19

### Patch Changes

- @pnpm/fetcher-base@15.0.7
- @pnpm/prepare-package@5.0.24
- @pnpm/worker@0.3.14

## 12.0.18

### Patch Changes

- @pnpm/prepare-package@5.0.23

## 12.0.17

### Patch Changes

- @pnpm/worker@0.3.13

## 12.0.16

### Patch Changes

- @pnpm/worker@0.3.12
- @pnpm/prepare-package@5.0.22
- @pnpm/fetcher-base@15.0.6

## 12.0.15

### Patch Changes

- @pnpm/prepare-package@5.0.21
- @pnpm/fetcher-base@15.0.5
- @pnpm/worker@0.3.11

## 12.0.14

### Patch Changes

- @pnpm/worker@0.3.10

## 12.0.13

### Patch Changes

- Updated dependencies [1e7bd4af3]
  - @pnpm/worker@0.3.9

## 12.0.12

### Patch Changes

- @pnpm/prepare-package@5.0.20
- @pnpm/worker@0.3.8

## 12.0.11

### Patch Changes

- @pnpm/prepare-package@5.0.19

## 12.0.10

### Patch Changes

- @pnpm/fetcher-base@15.0.4
- @pnpm/worker@0.3.7
- @pnpm/prepare-package@5.0.18

## 12.0.9

### Patch Changes

- @pnpm/prepare-package@5.0.17

## 12.0.8

### Patch Changes

- Updated dependencies [6390033cd]
  - @pnpm/worker@0.3.6
  - @pnpm/prepare-package@5.0.16
  - @pnpm/fetcher-base@15.0.3

## 12.0.7

### Patch Changes

- @pnpm/prepare-package@5.0.15

## 12.0.6

### Patch Changes

- @pnpm/prepare-package@5.0.14
- @pnpm/worker@0.3.5

## 12.0.5

### Patch Changes

- Updated dependencies [08b65ff78]
  - @pnpm/worker@0.3.4
  - @pnpm/prepare-package@5.0.13

## 12.0.4

### Patch Changes

- @pnpm/worker@0.3.3

## 12.0.3

### Patch Changes

- @pnpm/worker@0.3.2

## 12.0.2

### Patch Changes

- @pnpm/prepare-package@5.0.13
- @pnpm/fetcher-base@15.0.2
- @pnpm/worker@0.3.1

## 12.0.1

### Patch Changes

- Updated dependencies [17d2ddb05]
  - @pnpm/prepare-package@5.0.12

## 12.0.0

### Patch Changes

- Updated dependencies [9caa33d53]
  - @pnpm/worker@0.3.0
  - @pnpm/fetcher-base@15.0.1
  - @pnpm/prepare-package@5.0.11

## 11.0.1

### Patch Changes

- @pnpm/worker@0.2.1

## 11.0.0

### Patch Changes

- Updated dependencies [03cdccc6e]
- Updated dependencies [48dcd108c]
  - @pnpm/worker@0.2.0
  - @pnpm/fetcher-base@15.0.1
  - @pnpm/prepare-package@5.0.10

## 10.0.2

### Patch Changes

- @pnpm/worker@0.1.2
- @pnpm/prepare-package@5.0.9

## 10.0.1

### Patch Changes

- Updated dependencies [4a1a9431d]
  - @pnpm/fetcher-base@15.0.1
  - @pnpm/worker@0.1.1
  - @pnpm/prepare-package@5.0.9

## 10.0.0

### Major Changes

- 083bbf590: Breaking changes to the API.
- 70b2830ac: Breaking changes to the API.

### Patch Changes

- Updated dependencies [70b2830ac]
- Updated dependencies [083bbf590]
- Updated dependencies [083bbf590]
  - @pnpm/fetcher-base@15.0.0
  - @pnpm/worker@0.1.0
  - @pnpm/prepare-package@5.0.8

## 9.0.7

### Patch Changes

- @pnpm/prepare-package@5.0.7

## 9.0.6

### Patch Changes

- Updated dependencies [8452bb2d5]
  - @pnpm/prepare-package@5.0.6

## 9.0.5

### Patch Changes

- @pnpm/prepare-package@5.0.5
- @pnpm/fetcher-base@14.0.2

## 9.0.4

### Patch Changes

- @pnpm/prepare-package@5.0.4

## 9.0.3

### Patch Changes

- @pnpm/prepare-package@5.0.3

## 9.0.2

### Patch Changes

- @pnpm/prepare-package@5.0.2

## 9.0.1

### Patch Changes

- @pnpm/prepare-package@5.0.1
- @pnpm/fetcher-base@14.0.1

## 9.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/fetcher-base@14.0.0
  - @pnpm/prepare-package@5.0.0

## 8.0.2

### Patch Changes

- @pnpm/prepare-package@4.1.2

## 8.0.1

### Patch Changes

- @pnpm/fetcher-base@13.1.6
- @pnpm/prepare-package@4.1.1

## 8.0.0

### Major Changes

- c7b05cd9a: Added `@pnpm/logger` to the peer dependencies.

### Minor Changes

- c7b05cd9a: When ignoreScripts=true is passed to the fetcher, do not build git-hosted dependencies.

### Patch Changes

- Updated dependencies [c7b05cd9a]
  - @pnpm/prepare-package@4.1.0

## 7.0.1

### Patch Changes

- ec97a3105: Print more contextual information when a git-hosted package fails to be prepared for installation [#5847](https://github.com/pnpm/pnpm/pull/5847).
- Updated dependencies [ec97a3105]
- Updated dependencies [40a481840]
  - @pnpm/prepare-package@4.0.1

## 7.0.0

### Major Changes

- 339c0a704: A new required option added to the prepare package function: rawConfig. It is needed in order to create a proper environment for the package manager executed during the preparation of a git-hosted dependency.

### Patch Changes

- Updated dependencies [339c0a704]
- Updated dependencies [339c0a704]
  - @pnpm/prepare-package@4.0.0

## 6.0.4

### Patch Changes

- @pnpm/fetcher-base@13.1.5
- @pnpm/prepare-package@3.0.4

## 6.0.3

### Patch Changes

- @pnpm/prepare-package@3.0.3

## 6.0.2

### Patch Changes

- @pnpm/fetcher-base@13.1.4
- @pnpm/prepare-package@3.0.2

## 6.0.1

### Patch Changes

- @pnpm/fetcher-base@13.1.3
- @pnpm/prepare-package@3.0.1

## 6.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [f884689e0]
  - @pnpm/prepare-package@3.0.0

## 5.2.4

### Patch Changes

- @pnpm/prepare-package@2.0.11

## 5.2.3

### Patch Changes

- @pnpm/fetcher-base@13.1.2
- @pnpm/prepare-package@2.0.10

## 5.2.2

### Patch Changes

- @pnpm/fetcher-base@13.1.1
- @pnpm/prepare-package@2.0.9

## 5.2.1

### Patch Changes

- @pnpm/prepare-package@2.0.8

## 5.2.0

### Minor Changes

- 23984abd1: Add hook for adding custom fetchers.

### Patch Changes

- Updated dependencies [32915f0e4]
- Updated dependencies [23984abd1]
  - @pnpm/fetcher-base@13.1.0

## 5.1.7

### Patch Changes

- @pnpm/fetcher-base@13.0.2
- @pnpm/prepare-package@2.0.7

## 5.1.6

### Patch Changes

- @pnpm/fetcher-base@13.0.1
- @pnpm/prepare-package@2.0.6

## 5.1.5

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/fetcher-base@13.0.0
  - @pnpm/prepare-package@2.0.5

## 5.1.4

### Patch Changes

- Updated dependencies [0abfe1718]
  - @pnpm/fetcher-base@12.1.0
  - @pnpm/prepare-package@2.0.4

## 5.1.3

### Patch Changes

- @pnpm/fetcher-base@12.0.3
- @pnpm/prepare-package@2.0.3

## 5.1.2

### Patch Changes

- Updated dependencies [6756c2b02]
  - @pnpm/fetcher-base@12.0.2

## 5.1.1

### Patch Changes

- @pnpm/fetcher-base@12.0.1
- @pnpm/prepare-package@2.0.2

## 5.1.0

### Minor Changes

- c6463b9fd: feat(git-fetcher): shallow clone when fetching git resource

### Patch Changes

- @pnpm/prepare-package@2.0.1

## 5.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/fetcher-base@12.0.0
  - @pnpm/prepare-package@2.0.0

## 4.1.16

### Patch Changes

- @pnpm/prepare-package@1.0.13

## 4.1.15

### Patch Changes

- @pnpm/fetcher-base@11.1.6
- @pnpm/prepare-package@1.0.12

## 4.1.14

### Patch Changes

- @pnpm/fetcher-base@11.1.5
- @pnpm/prepare-package@1.0.11

## 4.1.13

### Patch Changes

- @pnpm/fetcher-base@11.1.4
- @pnpm/prepare-package@1.0.10

## 4.1.12

### Patch Changes

- Updated dependencies [eec4b195d]
  - @pnpm/prepare-package@1.0.9
  - @pnpm/fetcher-base@11.1.3

## 4.1.11

### Patch Changes

- @pnpm/fetcher-base@11.1.2
- @pnpm/prepare-package@1.0.8

## 4.1.10

### Patch Changes

- b13e4b452: Fixes a regression introduced in pnpm v6.23.3 via [#4044](https://github.com/pnpm/pnpm/pull/4044).

  The temporary directory to which the Git-hosted package is downloaded should not be removed prematurely [#4064](https://github.com/pnpm/pnpm/issues/4064).

## 4.1.9

### Patch Changes

- fb1a95a6c: The temporary directory should be removed after preparing the git-hosted package.
- Updated dependencies [fb1a95a6c]
  - @pnpm/prepare-package@1.0.7

## 4.1.8

### Patch Changes

- @pnpm/fetcher-base@11.1.1
- @pnpm/prepare-package@1.0.6

## 4.1.7

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/fetcher-base@11.1.0
  - @pnpm/prepare-package@1.0.5

## 4.1.6

### Patch Changes

- Updated dependencies [4a4d42d8f]
  - @pnpm/prepare-package@1.0.4

## 4.1.5

### Patch Changes

- 04b7f6086: Use safe-execa instead of execa to prevent binary planting attacks on Windows.

## 4.1.4

### Patch Changes

- @pnpm/fetcher-base@11.0.3
- @pnpm/prepare-package@1.0.3

## 4.1.3

### Patch Changes

- @pnpm/fetcher-base@11.0.2
- @pnpm/prepare-package@1.0.2

## 4.1.2

### Patch Changes

- @pnpm/fetcher-base@11.0.1
- @pnpm/prepare-package@1.0.1

## 4.1.1

### Patch Changes

- 3b147ced9: Do not remove the Git temporary directory because it might still be in the process of linking to the CAFS.

## 4.1.0

### Minor Changes

- e6a2654a2: Packages fetched from Git should have their `devDependencies` installed in case they have a `prepare` script.

### Patch Changes

- Updated dependencies [e6a2654a2]
- Updated dependencies [e6a2654a2]
- Updated dependencies [e6a2654a2]
  - @pnpm/prepare-package@1.0.0
  - @pnpm/fetcher-base@11.0.0

## 4.0.1

### Patch Changes

- @pnpm/fetcher-base@10.0.1

## 4.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
  - @pnpm/fetcher-base@10.0.0

## 3.0.13

### Patch Changes

- @pnpm/fetcher-base@9.0.4

## 3.0.12

### Patch Changes

- 32c9ef4be: execa updated to v5.

## 3.0.11

### Patch Changes

- @pnpm/fetcher-base@9.0.3

## 3.0.10

### Patch Changes

- @pnpm/fetcher-base@9.0.2

## 3.0.9

### Patch Changes

- 212671848: Update tempy to v1.
  - @pnpm/fetcher-base@9.0.1

## 3.0.8

### Patch Changes

- Updated dependencies [0a6544043]
  - @pnpm/fetcher-base@9.0.0

## 3.0.7

### Patch Changes

- 634dfd13b: tempy updated to v0.7.0.

## 3.0.6

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 3.0.5

### Patch Changes

- e8a853b5b: Update tempy to v0.6.0.

## 3.0.4

### Patch Changes

- @pnpm/fetcher-base@8.0.2

## 3.0.3

### Patch Changes

- @pnpm/fetcher-base@8.0.1

## 3.0.2

### Patch Changes

- Updated dependencies [bcd4aa1aa]
  - @pnpm/fetcher-base@8.0.0

## 3.0.1

### Patch Changes

- 187615f87: Adhere to the new FetchFunction API. cafs should be the first argument of the a fetch function.

## 3.0.0

### Major Changes

- b6a82072e: Using a content-addressable filesystem for storing packages.

### Minor Changes

- 42e6490d1: When a new package is being added to the store, its manifest is streamed in the memory. So instead of reading the manifest from the filesystem, we can parse the stream from the memory.

### Patch Changes

- Updated dependencies [f516d266c]
- Updated dependencies [b6a82072e]
- Updated dependencies [42e6490d1]
  - @pnpm/fetcher-base@7.0.0

## 2.0.11-alpha.4

### Patch Changes

- @pnpm/fetcher-base@6.0.1-alpha.3

## 3.0.0-alpha.2

### Minor Changes

- 42e6490d1: When a new package is being added to the store, its manifest is streamed in the memory. So instead of reading the manifest from the filesystem, we can parse the stream from the memory.

### Patch Changes

- Updated dependencies [42e6490d1]
  - @pnpm/fetcher-base@7.0.0-alpha.2

## 3.0.0-alpha.0

### Major Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/fetcher-base@7.0.0-alpha.0
