## 1102.0.15

### Patch Changes

- The env lockfile no longer pins `@pnpm/exe` alongside `pnpm` when the wanted pnpm version is 12 or newer. From v12 the unscoped `pnpm` package is itself the native executable, so `@pnpm/exe` is not published for it and resolving it would fail. The engine identity check now verifies the native binary through whichever package ships it.

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.1.2
  - @pnpm/config.pick-registry-for-package@1100.1.0
  - @pnpm/config.writer@1100.0.21
  - @pnpm/constants@1101.0.0
  - @pnpm/core-loggers@1100.3.2
  - @pnpm/deps.graph-hasher@1100.2.15
  - @pnpm/deps.path@1100.1.0
  - @pnpm/error@1100.1.1
  - @pnpm/fs.symlink-dependency@1100.0.17
  - @pnpm/installing.deps-resolver@1101.1.0
  - @pnpm/lockfile.fs@1100.2.0
  - @pnpm/lockfile.pruner@1100.0.19
  - @pnpm/lockfile.types@1100.0.19
  - @pnpm/lockfile.utils@1101.0.0
  - @pnpm/network.auth-header@1101.1.9
  - @pnpm/network.fetch@1100.1.11
  - @pnpm/pkg-manifest.reader@1100.0.15
  - @pnpm/resolving.npm-resolver@1103.1.0
  - @pnpm/store.controller@1102.0.11
  - @pnpm/store.controller-types@1101.1.0
  - @pnpm/types@1101.9.0
