## 1103.0.6

### Patch Changes

- pnpm no longer rewrites `packageManagerDependencies` in `pnpm-lock.yaml` when that block pins `@pnpm/exe` beside `pnpm`. The rewrite ran on every command, so `pnpm list` left a clean working tree dirty, and `pnpm version` then refused to run [#14926](https://github.com/pnpm/pnpm/issues/14926).

- Fixed `pnpm install` failing with `EEXIST` when a concurrent install cleared the file or directory that was occupying a symlink path. On Windows, a symlink another process is still holding is no longer moved aside and recreated.

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.0
  - @pnpm/config.pick-registry-for-package@1101.0.2
  - @pnpm/config.writer@1100.0.28
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/deps.graph-hasher@1100.3.4
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/fs.symlink-dependency@1100.0.21
  - @pnpm/installing.deps-resolver@1102.2.3
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.pruner@1100.0.25
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/network.auth-header@1101.1.14
  - @pnpm/network.fetch@1100.1.18
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/resolving.npm-resolver@1104.2.1
  - @pnpm/resolving.tarball-url@1101.1.2
  - @pnpm/store.controller@1102.2.0
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/types@1102.1.1
