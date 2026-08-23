## 1100.0.28

### Patch Changes

- The `importPackage` pnpmfile hook is deprecated. pnpm now prints a warning when a pnpmfile defines it, and the hook will be removed in the next major version. It also opts the installation out of the parallel package importer, making installation slower. If you rely on this hook, comment on [#14101](https://github.com/pnpm/pnpm/issues/14101).

- Updated dependencies:
  - @pnpm/core-loggers@1100.3.3
  - @pnpm/error@1100.1.3
  - @pnpm/hooks.types@1101.0.0
  - @pnpm/lockfile.types@1100.0.20
  - @pnpm/store.controller-types@1101.1.2
  - @pnpm/types@1102.0.0
