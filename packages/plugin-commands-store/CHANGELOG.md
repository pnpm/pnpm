# @pnpm/plugin-commands-store

## 2.0.0-alpha.3

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- 9596774f2: Store the package index files in the CAFS to reduce directory nesting.

### Patch Changes

- Updated dependencies [9596774f2]
- Updated dependencies [7852deea3]
  - @pnpm/cafs@1.0.0-alpha.3
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/get-context@1.2.2-alpha.0
  - @pnpm/store-connection-manager@0.2.32-alpha.3
  - @pnpm/cli-utils@0.4.5-alpha.0

## 1.0.11-alpha.2

### Patch Changes

- Updated dependencies [42e6490d1]
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/store-connection-manager@0.2.32-alpha.2

## 1.0.11-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/store-controller-types@8.0.0-alpha.1
  - @pnpm/store-connection-manager@0.2.32-alpha.1

## 2.0.0-alpha.0

### Major Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/store-controller-types@8.0.0-alpha.0
  - @pnpm/store-connection-manager@0.3.0-alpha.0

## 1.0.10

### Patch Changes

- 907c63a48: Update `@pnpm/store-path`.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/store-connection-manager@0.2.31
  - @pnpm/get-context@1.2.1
  - @pnpm/cli-utils@0.4.4
