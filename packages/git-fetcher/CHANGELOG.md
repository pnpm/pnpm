# @pnpm/git-fetcher

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
