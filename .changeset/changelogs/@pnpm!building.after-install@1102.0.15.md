## 1102.0.15

### Patch Changes

- Security: `pnpm rebuild` now refuses a lockfile whose `packages` key carries a path traversal in the package name (e.g. `../../../escaped@1.0.0`), instead of running that package's lifecycle scripts and linking its bins in a directory outside the virtual store. Such a name is rejected with `ERR_PNPM_INVALID_DEPENDENCY_NAME`.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.25
  - @pnpm/building.pkg-requires-build@1100.0.14
  - @pnpm/building.policy@1100.0.18
  - @pnpm/config.normalize-registries@1100.1.0
  - @pnpm/config.reader@1101.16.0
  - @pnpm/constants@1101.0.0
  - @pnpm/core-loggers@1100.3.2
  - @pnpm/deps.graph-hasher@1100.2.15
  - @pnpm/deps.path@1100.1.0
  - @pnpm/error@1100.1.1
  - @pnpm/exec.lifecycle@1100.1.11
  - @pnpm/fs.symlink-dependency@1100.0.17
  - @pnpm/installing.context@1100.1.0
  - @pnpm/installing.modules-yaml@1100.0.15
  - @pnpm/lockfile.types@1100.0.19
  - @pnpm/lockfile.utils@1101.0.0
  - @pnpm/lockfile.walker@1100.0.19
  - @pnpm/pkg-manifest.reader@1100.0.15
  - @pnpm/store.cafs@1100.1.18
  - @pnpm/store.connection-manager@1100.3.15
  - @pnpm/store.controller-types@1101.1.0
  - @pnpm/store.index@1100.2.3
  - @pnpm/types@1101.9.0
