## 1100.1.34

### Patch Changes

- With `dedupeDirectDeps`, `pnpm install` now compares `link:` dependencies by the directory they point at. A project's `link:` dependency is removed when the workspace root links the same directory under a different path. It is kept when the root's `link:` dependency has the same text but points at another directory.

  With `nodeLinker: hoisted` and `dedupeDirectDeps`, `pnpm install` now removes a project's `link:` dependency that the workspace root provides.

  With `nodeLinker: hoisted`, `pnpm install` now reports each `link:` dependency it removes.

- With `dedupeDirectDeps`, `pnpm install` reports a project dependency that the workspace root provides as removed only once, when it removes the project's link [pnpm/tasks#293](https://github.com/pnpm/tasks/issues/293).

- Updated dependencies:
  - @pnpm/bins.remover@1100.0.32
  - @pnpm/deps.path@1101.0.9
  - @pnpm/lockfile.filtering@1100.2.17
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/store.controller-types@1101.3.6
