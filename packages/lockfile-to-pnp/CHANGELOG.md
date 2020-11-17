# @pnpm/lockfile-to-pnp

## 0.3.9

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/config@11.9.0
  - @pnpm/lockfile-utils@2.0.20

## 0.3.8

### Patch Changes

- Updated dependencies [fcc1c7100]
  - @pnpm/config@11.8.0

## 0.3.7

### Patch Changes

- @pnpm/config@11.7.2
- @pnpm/lockfile-file@3.1.1
- @pnpm/read-project-manifest@1.1.5

## 0.3.6

### Patch Changes

- Updated dependencies [3776b5a52]
  - @pnpm/lockfile-file@3.1.0

## 0.3.5

### Patch Changes

- Updated dependencies [dbcc6c96f]
- Updated dependencies [09492b7b4]
  - @pnpm/lockfile-file@3.0.18
  - @pnpm/read-project-manifest@1.1.4

## 0.3.4

### Patch Changes

- 60e01bd1d: @pnpm/logger should not be a prod dependency because it is a peer dependency.
- Updated dependencies [39142e2ad]
- Updated dependencies [aa6bc4f95]
  - dependency-path@5.0.6
  - @pnpm/lockfile-file@3.0.17
  - @pnpm/lockfile-utils@2.0.19
  - @pnpm/read-project-manifest@1.1.3

## 0.3.3

### Patch Changes

- @pnpm/lockfile-file@3.0.16
- @pnpm/lockfile-utils@2.0.18
- @pnpm/config@11.7.1
- dependency-path@5.0.5
- @pnpm/read-project-manifest@1.1.2

## 0.3.2

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/config@11.7.0

## 0.3.1

### Patch Changes

- @pnpm/lockfile-file@3.0.15
- @pnpm/lockfile-utils@2.0.17
- @pnpm/config@11.6.1
- dependency-path@5.0.4
- @pnpm/read-project-manifest@1.1.1

## 0.3.0

### Minor Changes

- f591fdeeb: CLI command removed.

### Patch Changes

- Updated dependencies [f591fdeeb]
  - @pnpm/config@11.6.0

## 0.2.0

### Minor Changes

- faac0745b: Rename lockfileDirectory to lockfileDir.

### Patch Changes

- faac0745b: Always set the packageLocation correctly.
- Updated dependencies [2762781cc]
  - @pnpm/read-project-manifest@1.1.0

## 0.1.4

### Patch Changes

- 119da15e9: pathLocation of workspace project should always start with "./"

## 0.1.3

### Patch Changes

- 646c7868b: `@pnpm/logger` should be a prod dependency as lockfile-to-pnp is a standalone CLI app.

## 0.1.2

### Patch Changes

- c3d34232c: Normalize `packageLocation` path.
- c3d34232c: Use correct return type for `lockfileToPackageRegistry`.
- Updated dependencies [74914c178]
  - @pnpm/config@11.5.0

## 0.1.1

### Patch Changes

- ee91574b7: packageLocation should be a relative path.

## 0.1.0

### Minor Changes

- c9f0c7764: Initial version.

### Patch Changes

- Updated dependencies [23cf3c88b]
  - @pnpm/config@11.4.0
