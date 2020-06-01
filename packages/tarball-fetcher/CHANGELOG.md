# @pnpm/tarball-fetcher

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
