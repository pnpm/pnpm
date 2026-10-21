## 1100.0.39

### Patch Changes

- When `pnpm install` resolves a dependency again, it now reuses a version that the install already resolved for the same package, as a fresh install does. Before, it kept the version the lockfile pinned and added a second copy of the package. A locked version still stays when nothing else in the install prefers another version. To pin a transitive dependency on purpose, use `overrides` [#17084](https://github.com/pnpm/pnpm/issues/17084).

- Updated dependencies:
  - @pnpm/lockfile.utils@1102.1.7
  - @pnpm/pkg-manifest.utils@1100.4.10
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/types@1102.1.2
