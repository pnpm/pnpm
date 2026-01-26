# @pnpm/fetching.binary-fetcher

## 1005.0.3

### Patch Changes

- Updated dependencies [a484cea]
  - @pnpm/fetching-types@1000.2.1
  - @pnpm/worker@1000.6.3

## 1005.0.2

### Patch Changes

- 5c382f0: Fix path traversal vulnerability in binary fetcher ZIP extraction

  - Validate ZIP entry paths before extraction to prevent writing files outside target directory
  - Validate BinaryResolution.prefix (basename) to prevent directory escape via crafted prefix
  - Both attack vectors now throw `ERR_PNPM_PATH_TRAVERSAL` error
  - @pnpm/fetcher-base@1001.2.2
  - @pnpm/worker@1000.6.2

## 1005.0.1

### Patch Changes

- @pnpm/fetcher-base@1001.2.1
- @pnpm/worker@1000.6.1

## 1005.0.0

### Patch Changes

- 914f2e5: Runtime dependencies (node, bun, deno) are now added to the store with a package.json file.
- Updated dependencies [914f2e5]
  - @pnpm/fetcher-base@1001.2.0
  - @pnpm/worker@1000.6.0

## 1004.0.0

### Patch Changes

- Updated dependencies [4077539]
- Updated dependencies [b7d3ec6]
  - @pnpm/fetcher-base@1001.1.0
  - @pnpm/worker@1000.5.0

## 1003.0.1

### Patch Changes

- @pnpm/fetcher-base@1001.0.6
- @pnpm/worker@1000.4.1

## 1003.0.0

### Patch Changes

- Updated dependencies [d42558f]
- Updated dependencies [463f30c]
  - @pnpm/worker@1000.4.0

## 1002.0.3

### Patch Changes

- @pnpm/fetcher-base@1001.0.5
- @pnpm/worker@1000.3.3

## 1002.0.2

### Patch Changes

- @pnpm/worker@1000.3.2
- @pnpm/fetcher-base@1001.0.4

## 1002.0.1

### Patch Changes

- @pnpm/fetcher-base@1001.0.3
- @pnpm/worker@1000.3.1

## 1002.0.0

### Patch Changes

- Updated dependencies [8993f68]
  - @pnpm/worker@1000.3.0
  - @pnpm/fetcher-base@1001.0.2

## 1001.0.0

### Patch Changes

- Updated dependencies [06d2160]
  - @pnpm/worker@1000.2.0

## 1000.0.3

### Patch Changes

- @pnpm/error@1000.0.5
- @pnpm/worker@1000.1.13

## 1000.0.2

### Patch Changes

- @pnpm/fetcher-base@1001.0.1
- @pnpm/worker@1000.1.12

## 1000.0.1

### Patch Changes

- 2b0d35f: `@pnpm/worker` should always be a peer dependency.

## 1000.0.0

### Major Changes

- d1edf73: Added support for binary fetcher.

### Patch Changes

- Updated dependencies [d1edf73]
  - @pnpm/fetcher-base@1001.0.0
  - @pnpm/error@1000.0.4
  - @pnpm/worker@1000.1.11
