# @pnpm/reviewing.dependencies-hierarchy

## 2.0.2

### Patch Changes

- Updated dependencies [72ba638e3]
  - @pnpm/lockfile-utils@7.0.0

## 2.0.1

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
- Updated dependencies [94f94eed6]
  - @pnpm/dependency-path@2.1.0
  - @pnpm/lockfile-file@8.0.1
  - @pnpm/lockfile-utils@6.0.1

## 2.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [c92936158]
- Updated dependencies [158d8cf22]
- Updated dependencies [ca8f51e60]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [0e26acb0f]
- Updated dependencies [417c8ac59]
  - @pnpm/lockfile-utils@6.0.0
  - @pnpm/lockfile-file@8.0.0
  - @pnpm/dependency-path@2.0.0
  - @pnpm/read-package-json@8.0.0
  - @pnpm/normalize-registries@5.0.0
  - @pnpm/modules-yaml@12.0.0
  - @pnpm/read-modules-dir@6.0.0
  - @pnpm/types@9.0.0

## 1.2.5

### Patch Changes

- Updated dependencies [787c43dcc]
  - @pnpm/lockfile-file@7.0.6

## 1.2.4

### Patch Changes

- Updated dependencies [ed946c73e]
  - @pnpm/lockfile-file@7.0.5

## 1.2.3

### Patch Changes

- @pnpm/lockfile-utils@5.0.7

## 1.2.2

### Patch Changes

- 19e823bea: Show correct path info for dependenciesHierarchy tree
- Updated dependencies [d89d7a078]
  - @pnpm/dependency-path@1.1.3
  - @pnpm/lockfile-file@7.0.4
  - @pnpm/lockfile-utils@5.0.6

## 1.2.1

### Patch Changes

- Updated dependencies [9247f6781]
  - @pnpm/dependency-path@1.1.2
  - @pnpm/lockfile-file@7.0.3
  - @pnpm/lockfile-utils@5.0.5

## 1.2.0

### Minor Changes

- 94ef3299e: Show dependency paths info in `pnpm audit` output [#3073](https://github.com/pnpm/pnpm/issues/3073)

## 1.1.3

### Patch Changes

- Updated dependencies [9a68ebbae]
  - @pnpm/lockfile-file@7.0.2

## 1.1.2

### Patch Changes

- Updated dependencies [0f6e95872]
  - @pnpm/dependency-path@1.1.1
  - @pnpm/lockfile-file@7.0.1
  - @pnpm/lockfile-utils@5.0.4

## 1.1.1

### Patch Changes

- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
  - @pnpm/lockfile-file@7.0.0
  - @pnpm/dependency-path@1.1.0
  - @pnpm/lockfile-utils@5.0.3
  - @pnpm/read-package-json@7.0.5

## 1.1.0

### Minor Changes

- 395a33a50: The `path` field for direct dependencies returned from `buildDependenciesHierarchy` was incorrect if the dependency used the `workspace:` or `link:` protocols.
- 395a33a50: The `pnpm list` and `pnpm why` commands will now look through transitive dependencies of `workspace:` packages. A new `--only-projects` flag is available to only print `workspace:` packages.

### Patch Changes

- 7853a26e1: Fix a situation where `pnpm list` and `pnpm why` may not respect the `--depth` argument.

## 1.0.1

### Patch Changes

- Updated dependencies [b77651d14]
- Updated dependencies [2458741fa]
  - @pnpm/types@8.10.0
  - @pnpm/modules-yaml@11.1.0
  - @pnpm/normalize-registries@4.0.3
  - @pnpm/lockfile-file@6.0.5
  - @pnpm/lockfile-utils@5.0.2
  - @pnpm/dependency-path@1.0.1
  - @pnpm/read-package-json@7.0.4

## 1.0.0

### Major Changes

- 313702d76: Project renamed from `dependencies-hierarchy` to `@pnpm/reviewing.dependencies-hierarchy`.

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/dependency-path@1.0.0
  - @pnpm/lockfile-file@6.0.4
  - @pnpm/lockfile-utils@5.0.1
