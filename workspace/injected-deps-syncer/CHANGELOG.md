# @pnpm/workspace.injected-deps-syncer

## 1100.0.4

### Patch Changes

- @pnpm/fetching.directory-fetcher@1100.0.4

## 1100.0.3

### Patch Changes

- Updated dependencies [e03e8f4]
  - @pnpm/fetching.directory-fetcher@1100.0.3

## 1100.0.2

### Patch Changes

- Updated dependencies [cee550a]
  - @pnpm/workspace.projects-reader@1101.0.0
  - @pnpm/bins.linker@1100.0.2
  - @pnpm/fetching.directory-fetcher@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/bins.linker@1100.0.1
  - @pnpm/fetching.directory-fetcher@1100.0.1
  - @pnpm/installing.modules-yaml@1100.0.1
  - @pnpm/pkg-manifest.reader@1100.0.1
  - @pnpm/workspace.projects-reader@1100.0.1

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Patch Changes

- Updated dependencies [449dacf]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [3cfffaa]
- Updated dependencies [05fb1ae]
- Updated dependencies [491a84f]
- Updated dependencies [62f760e]
- Updated dependencies [6e9cad3]
- Updated dependencies [cb228c9]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [4a36b9a]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [f40177f]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [3cfffaa]
- Updated dependencies [efb48dc]
  - @pnpm/bins.linker@1001.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/installing.modules-yaml@1001.0.0
  - @pnpm/pkg-manifest.reader@1001.0.0
  - @pnpm/fetching.directory-fetcher@1001.0.0
  - @pnpm/workspace.projects-reader@1001.0.0
  - @pnpm/error@1001.0.0

## 1000.0.17

### Patch Changes

- @pnpm/workspace.find-packages@1000.0.43

## 1000.0.16

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0
  - @pnpm/directory-fetcher@1000.1.14
  - @pnpm/link-bins@1000.2.6
  - @pnpm/modules-yaml@1000.3.6
  - @pnpm/read-package-json@1000.1.2
  - @pnpm/workspace.find-packages@1000.0.42

## 1000.0.15

### Patch Changes

- 6089939: Sync bin links after injected dependencies are updated by build scripts. This ensures that binaries created during build processes are properly linked and accessible to consuming projects [#10057](https://github.com/pnpm/pnpm/issues/10057).
- Updated dependencies [a8797c4]
  - @pnpm/link-bins@1000.2.5
  - @pnpm/workspace.find-packages@1000.0.41

## 1000.0.14

### Patch Changes

- @pnpm/directory-fetcher@1000.1.13

## 1000.0.13

### Patch Changes

- @pnpm/error@1000.0.5
- @pnpm/directory-fetcher@1000.1.12

## 1000.0.12

### Patch Changes

- @pnpm/directory-fetcher@1000.1.11
- @pnpm/modules-yaml@1000.3.5

## 1000.0.11

### Patch Changes

- @pnpm/error@1000.0.4
- @pnpm/directory-fetcher@1000.1.10

## 1000.0.10

### Patch Changes

- @pnpm/directory-fetcher@1000.1.9
- @pnpm/modules-yaml@1000.3.4
- @pnpm/error@1000.0.3

## 1000.0.9

### Patch Changes

- @pnpm/directory-fetcher@1000.1.8

## 1000.0.8

### Patch Changes

- 09cf46f: Update `@pnpm/logger` in peer dependencies.
- Updated dependencies [09cf46f]
  - @pnpm/directory-fetcher@1000.1.7
  - @pnpm/modules-yaml@1000.3.3

## 1000.0.7

### Patch Changes

- Updated dependencies [8a9f3a4]
  - @pnpm/logger@1001.0.0
  - @pnpm/directory-fetcher@1000.1.6
  - @pnpm/modules-yaml@1000.3.2

## 1000.0.6

### Patch Changes

- @pnpm/directory-fetcher@1000.1.5

## 1000.0.5

### Patch Changes

- @pnpm/directory-fetcher@1000.1.4
- @pnpm/modules-yaml@1000.3.1

## 1000.0.4

### Patch Changes

- Updated dependencies [64f6b4f]
  - @pnpm/modules-yaml@1000.3.0
  - @pnpm/directory-fetcher@1000.1.3

## 1000.0.3

### Patch Changes

- Updated dependencies [d612dcf]
- Updated dependencies [d612dcf]
  - @pnpm/modules-yaml@1000.2.0
  - @pnpm/directory-fetcher@1000.1.2

## 1000.0.2

### Patch Changes

- 9904675: `@pnpm/logger` should be a peer dependency.

## 1000.0.1

### Patch Changes

- @pnpm/directory-fetcher@1000.1.1
- @pnpm/modules-yaml@1000.1.4

## 1000.0.0

### Major Changes

- e32b1a2: Added support for automatically syncing files of injected workspace packages after `pnpm run` [#9081](https://github.com/pnpm/pnpm/issues/9081). Use the `sync-injected-deps-after-scripts` setting to specify which scripts build the workspace package. This tells pnpm when syncing is needed. The setting should be defined in a `.npmrc` file at the root of the workspace. Example:

  ```ini
  sync-injected-deps-after-scripts[]=compile
  ```

- e32b1a2: Initial Release.

### Patch Changes

- Updated dependencies [e32b1a2]
  - @pnpm/directory-fetcher@1000.1.0
  - @pnpm/modules-yaml@1000.1.3
