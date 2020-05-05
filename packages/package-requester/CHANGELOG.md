# @pnpm/package-requester

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
