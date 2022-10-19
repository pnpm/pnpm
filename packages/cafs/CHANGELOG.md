# @pnpm/cafs

## 5.0.1

### Patch Changes

- @pnpm/fetcher-base@13.1.3
- @pnpm/store-controller-types@14.1.4

## 5.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

## 4.3.2

### Patch Changes

- @pnpm/fetcher-base@13.1.2
- @pnpm/store-controller-types@14.1.3

## 4.3.1

### Patch Changes

- @pnpm/fetcher-base@13.1.1
- @pnpm/store-controller-types@14.1.2

## 4.3.0

### Minor Changes

- 745143e79: Extend cafs with `getFilePathByModeInCafs`.

### Patch Changes

- @pnpm/fetcher-base@13.1.0
- @pnpm/store-controller-types@14.1.1

## 4.2.1

### Patch Changes

- dbac0ca01: Update ssri to v9.

## 4.2.0

### Minor Changes

- 32915f0e4: Refactor cafs types into separate package and add additional properties including `cafsDir` and `getFilePathInCafs`.

### Patch Changes

- Updated dependencies [32915f0e4]
- Updated dependencies [23984abd1]
  - @pnpm/fetcher-base@13.1.0
  - @pnpm/store-controller-types@14.1.1

## 4.1.0

### Minor Changes

- c191ca7bf: Fix bug where the package manifest was not resolved if `verifyStoreIntegrity` is set to `false`.

## 4.0.9

### Patch Changes

- 39c040127: upgrade various dependencies
- Updated dependencies [65c4260de]
  - @pnpm/store-controller-types@14.1.0

## 4.0.8

### Patch Changes

- @pnpm/fetcher-base@13.0.2
- @pnpm/store-controller-types@14.0.2

## 4.0.7

### Patch Changes

- @pnpm/fetcher-base@13.0.1
- @pnpm/store-controller-types@14.0.1

## 4.0.6

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/fetcher-base@13.0.0
  - @pnpm/store-controller-types@14.0.0

## 4.0.5

### Patch Changes

- Updated dependencies [0abfe1718]
  - @pnpm/fetcher-base@12.1.0
  - @pnpm/store-controller-types@13.0.4

## 4.0.4

### Patch Changes

- @pnpm/fetcher-base@12.0.3
- @pnpm/store-controller-types@13.0.3

## 4.0.3

### Patch Changes

- 6756c2b02: It should be possible to install a git-hosted package that has no `package.json` file [#4822](https://github.com/pnpm/pnpm/issues/4822).
- Updated dependencies [6756c2b02]
  - @pnpm/fetcher-base@12.0.2
  - @pnpm/store-controller-types@13.0.2

## 4.0.2

### Patch Changes

- cadefe5b6: Track the number of integrity checks.

## 4.0.1

### Patch Changes

- @pnpm/fetcher-base@12.0.1
- @pnpm/store-controller-types@13.0.1

## 4.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/fetcher-base@12.0.0
  - @pnpm/graceful-fs@2.0.0
  - @pnpm/store-controller-types@13.0.0

## 3.0.15

### Patch Changes

- Updated dependencies [5c525db13]
  - @pnpm/store-controller-types@12.0.0

## 3.0.14

### Patch Changes

- @pnpm/fetcher-base@11.1.6
- @pnpm/store-controller-types@11.0.12

## 3.0.13

### Patch Changes

- @pnpm/fetcher-base@11.1.5
- @pnpm/store-controller-types@11.0.11

## 3.0.12

### Patch Changes

- @pnpm/fetcher-base@11.1.4
- @pnpm/store-controller-types@11.0.10

## 3.0.11

### Patch Changes

- @pnpm/fetcher-base@11.1.3
- @pnpm/store-controller-types@11.0.9

## 3.0.10

### Patch Changes

- @pnpm/fetcher-base@11.1.2
- @pnpm/store-controller-types@11.0.8

## 3.0.9

### Patch Changes

- @pnpm/fetcher-base@11.1.1
- @pnpm/store-controller-types@11.0.7

## 3.0.8

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/fetcher-base@11.1.0
  - @pnpm/store-controller-types@11.0.6

## 3.0.7

### Patch Changes

- @pnpm/fetcher-base@11.0.3
- @pnpm/store-controller-types@11.0.5

## 3.0.6

### Patch Changes

- @pnpm/fetcher-base@11.0.2
- @pnpm/store-controller-types@11.0.4

## 3.0.5

### Patch Changes

- @pnpm/fetcher-base@11.0.1
- @pnpm/store-controller-types@11.0.3

## 3.0.4

### Patch Changes

- ef0ca24be: Use graceful-fs for reading files.
- Updated dependencies [a2aeeef88]
  - @pnpm/graceful-fs@1.0.0

## 3.0.3

### Patch Changes

- Updated dependencies [e6a2654a2]
- Updated dependencies [e6a2654a2]
  - @pnpm/fetcher-base@11.0.0
  - @pnpm/store-controller-types@11.0.2

## 3.0.2

### Patch Changes

- @pnpm/fetcher-base@10.0.1
- @pnpm/store-controller-types@11.0.1

## 3.0.1

### Patch Changes

- 6f198457d: Update rename-overwrite.

## 3.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- 83645c8ed: Update ssri.
- Updated dependencies [97b986fbc]
  - @pnpm/fetcher-base@10.0.0
  - @pnpm/store-controller-types@11.0.0

## 2.1.0

### Minor Changes

- 8d1dfa89c: New fields added to `PackageFilesIndex`: `name` and `version`.

### Patch Changes

- Updated dependencies [8d1dfa89c]
  - @pnpm/store-controller-types@10.0.0

## 2.0.5

### Patch Changes

- @pnpm/fetcher-base@9.0.4
- @pnpm/store-controller-types@9.2.1

## 2.0.4

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/store-controller-types@9.2.0
  - @pnpm/fetcher-base@9.0.3

## 2.0.3

### Patch Changes

- b3059f4f8: Don't unpack file duplicates to the content-addressable store.

## 2.0.2

### Patch Changes

- @pnpm/fetcher-base@9.0.2
- @pnpm/store-controller-types@9.1.2

## 2.0.1

### Patch Changes

- @pnpm/fetcher-base@9.0.1
- @pnpm/store-controller-types@9.1.1

## 2.0.0

### Major Changes

- 0a6544043: `generatingIntegrity` replaced with `writeResult`. When files are added to the store, the store returns not only the file's integrity as a result, but also the exact time when the file's content was verified with its integrity.

### Minor Changes

- 0a6544043: If a file in the store was never modified, we can skip checking its integrity.

### Patch Changes

- Updated dependencies [0a6544043]
- Updated dependencies [0a6544043]
  - @pnpm/store-controller-types@9.1.0
  - @pnpm/fetcher-base@9.0.0

## 1.0.8

### Patch Changes

- Updated dependencies [86cd72de3]
  - @pnpm/store-controller-types@9.0.0

## 1.0.7

### Patch Changes

- 1525fff4c: Update get-stream to v6.

## 1.0.6

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 1.0.5

### Patch Changes

- @pnpm/fetcher-base@8.0.2
- @pnpm/store-controller-types@8.0.2

## 1.0.4

### Patch Changes

- @pnpm/fetcher-base@8.0.1
- @pnpm/store-controller-types@8.0.1

## 1.0.3

### Patch Changes

- 492805ee3: Strip byte order mark (BOM) before parsing the content of a package manifest (package.json).

## 1.0.2

### Patch Changes

- d3ddd023c: Update p-limit to v3.

## 1.0.1

### Patch Changes

- Updated dependencies [bcd4aa1aa]
  - @pnpm/fetcher-base@8.0.0

## 1.0.0

### Major Changes

- 9596774f2: Store the package index files in the CAFS to reduce directory nesting.
- 7852deea3: Instead of creating a separate subdir for executables in the content-addressable storage, use the directory where all the files are stored but suffix the executable files with `-exec`. Also suffix the package index files with `-index.json`.
- b6a82072e: Project created.
- b6a82072e: Using a content-addressable filesystem for storing packages.
- 471149e66: Change the format of the package index file. Move all the files info into a "files" property.

### Minor Changes

- f516d266c: Executables are saved into a separate directory inside the content-addressable storage.
- a5febb913: sideEffects property added to files index file.
- 42e6490d1: When a new package is being added to the store, its manifest is streamed in the memory. So instead of reading the manifest from the filesystem, we can parse the stream from the memory.

### Patch Changes

- c207d994f: Update rename-overwrite to v3.
- Updated dependencies [16d1ac0fd]
- Updated dependencies [f516d266c]
- Updated dependencies [da091c711]
- Updated dependencies [42e6490d1]
- Updated dependencies [a5febb913]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [42e6490d1]
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/fetcher-base@7.0.0

## 1.0.0-alpha.5

### Minor Changes

- a5febb913: sideEffects property added to files index file.

### Patch Changes

- Updated dependencies [16d1ac0fd]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/store-controller-types@8.0.0-alpha.4

## 1.0.0-alpha.4

### Major Changes

- 471149e6: Change the format of the package index file. Move all the files info into a "files" property.

### Patch Changes

- @pnpm/fetcher-base@6.0.1-alpha.3

## 1.0.0-alpha.3

### Major Changes

- 9596774f2: Store the package index files in the CAFS to reduce directory nesting.
- 7852deea3: Instead of creating a separate subdir for executables in the content-addressable storage, use the directory where all the files are stored but suffix the executable files with `-exec`. Also suffix the package index files with `-index.json`.

## 1.0.0-alpha.2

### Minor Changes

- 42e6490d1: When a new package is being added to the store, its manifest is streamed in the memory. So instead of reading the manifest from the filesystem, we can parse the stream from the memory.

### Patch Changes

- c207d994f: Update rename-overwrite to v3.
- Updated dependencies [42e6490d1]
  - @pnpm/fetcher-base@7.0.0-alpha.2

## 1.0.0-alpha.1

### Minor Changes

- 4f62d0383: Executables are saved into a separate directory inside the content-addressable storage.

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/fetcher-base@7.0.0-alpha.1

## 1.0.0-alpha.0

### Major Changes

- 91c4b5954: Project created.
- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/fetcher-base@7.0.0-alpha.0
