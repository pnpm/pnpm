# @pnpm/reviewing.dependencies-hierarchy

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
