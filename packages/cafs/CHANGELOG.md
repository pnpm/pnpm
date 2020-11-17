# @pnpm/cafs

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
