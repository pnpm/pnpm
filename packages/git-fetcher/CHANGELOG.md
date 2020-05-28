# @pnpm/git-fetcher

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
