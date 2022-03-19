# @pnpm/fetcher-base

## 12.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/resolver-base@9.0.0

## 11.1.6

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/resolver-base@8.1.6

## 11.1.5

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/resolver-base@8.1.5

## 11.1.4

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/resolver-base@8.1.4

## 11.1.3

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/resolver-base@8.1.3

## 11.1.2

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/resolver-base@8.1.2

## 11.1.1

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/resolver-base@8.1.1

## 11.1.0

### Minor Changes

- 4ab87844a: The files response can point to files that are not in the global content-addressable store. In this case, the response will contain a `local: true` property, and the structure of `filesIndex` will be just a `Record<string, string>`.
- 4ab87844a: New optional property is added to `PackageFilesResponse` for specifying how the package needs to be imported to the modules directory. Should it be hard linked, copied, or cloned.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/resolver-base@8.1.0

## 11.0.3

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/resolver-base@8.0.4

## 11.0.2

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/resolver-base@8.0.3

## 11.0.1

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/resolver-base@8.0.2

## 11.0.0

### Major Changes

- e6a2654a2: `importPackage()` and `tempDir()` added to the `Cafs` type.

### Minor Changes

- e6a2654a2: `PackageFileInfo`, `PackageFilesResponse`, and `ImportPackageFunction` types added.

## 10.0.1

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/resolver-base@8.0.1

## 10.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
  - @pnpm/resolver-base@8.0.0
  - @pnpm/types@7.0.0

## 9.0.4

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/resolver-base@7.1.1

## 9.0.3

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/resolver-base@7.1.0

## 9.0.2

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/resolver-base@7.0.5

## 9.0.1

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/resolver-base@7.0.4

## 9.0.0

### Major Changes

- 0a6544043: `generatingIntegrity` replaced with `writeResult`. When files are added to the store, the store returns not only the file's integrity as a result, but also the exact time when the file's content was verified with its integrity.

## 8.0.2

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/resolver-base@7.0.3

## 8.0.1

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/resolver-base@7.0.2

## 8.0.0

### Major Changes

- bcd4aa1aa: Remove `cachedTarballLocation` from `FetchOptions`.

## 7.0.0

### Major Changes

- b6a82072e: Using a content-addressable filesystem for storing packages.

### Minor Changes

- f516d266c: Executables are saved into a separate directory inside the content-addressable storage.
- 42e6490d1: When a new package is being added to the store, its manifest is streamed in the memory. So instead of reading the manifest from the filesystem, we can parse the stream from the memory.

### Patch Changes

- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0
  - @pnpm/resolver-base@7.0.1

## 7.0.0-alpha.3

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0

## 7.0.0-alpha.2

### Minor Changes

- 42e6490d1: When a new package is being added to the store, its manifest is streamed in the memory. So instead of reading the manifest from the filesystem, we can parse the stream from the memory.

## 7.0.0-alpha.1

### Minor Changes

- 4f62d0383: Executables are saved into a separate directory inside the content-addressable storage.

## 7.0.0-alpha.0

### Major Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.
