# dependency-path

## 8.0.9

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0

## 8.0.8

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1

## 8.0.7

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0

## 8.0.6

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0

## 8.0.5

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0

## 8.0.4

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0

## 8.0.3

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0

## 8.0.2

### Patch Changes

- 6c418943c: Fix `tryGetPackageId()`, it should parse correctly a dependency path that has peer dependency names with underscores.

## 8.0.1

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0

## 8.0.0

### Major Changes

- 20e2f235d: Add a unique prefix to any directory name inside the virtual store that has non-lowercase characters. This is important to avoid conflicts in case insensitive filesystems.

## 7.0.1

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0

## 7.0.0

### Major Changes

- 9ceab68f0: Use + instead of # to escape / in paths.

## 6.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.
- f2bb5cbeb: All packages inside the virtual store directory should be on the same depth. Instead of subdirectories, one directory is used with # instead of slashes.

### Patch Changes

- e4efddbd2: Don't use ":" in path to dependency.
- Updated dependencies [97b986fbc]
  - @pnpm/types@7.0.0

## 5.1.1

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0

## 5.1.0

### Minor Changes

- e27dcf0dc: Add depPathToFilename().

## 5.0.6

### Patch Changes

- 39142e2ad: Update encode-registry to v3.

## 5.0.5

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1

## 5.0.4

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0

## 5.0.3

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 5.0.2

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0

## 5.0.1

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0

## 5.0.0

### Major Changes

- 41d92948b: relative() should always remove the registry from the IDs start.

## 4.0.7

### Patch Changes

- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0

## 4.0.7-alpha.0

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
