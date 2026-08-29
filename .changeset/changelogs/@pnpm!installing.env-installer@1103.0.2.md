## 1103.0.2

### Patch Changes

- A `devEngines.packageManager` range pin on pnpm is now recorded in `pnpm-lock.yaml`'s `packageManagerDependencies` when the running pnpm already satisfies it, using the running version and keeping the range as the recorded specifier. Previously only an exact pin — or a range resolved on the way through a version switch — reached the lockfile, so a range pin written by hand (or by any tool other than `pnpm add` / `pnpm self-update`) left the project without the shared resolution the pin exists to provide.

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.1.5
  - @pnpm/config.pick-registry-for-package@1101.0.1
  - @pnpm/config.writer@1100.0.24
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/deps.graph-hasher@1100.3.0
  - @pnpm/deps.path@1101.0.1
  - @pnpm/fs.symlink-dependency@1100.0.19
  - @pnpm/installing.deps-resolver@1102.1.0
  - @pnpm/lockfile.fs@1100.2.5
  - @pnpm/lockfile.pruner@1100.0.21
  - @pnpm/lockfile.types@1100.1.0
  - @pnpm/lockfile.utils@1102.1.0
  - @pnpm/network.auth-header@1101.1.12
  - @pnpm/network.fetch@1100.1.14
  - @pnpm/pkg-manifest.reader@1100.0.18
  - @pnpm/resolving.npm-resolver@1104.1.0
  - @pnpm/resolving.tarball-url@1101.1.0
  - @pnpm/store.controller@1102.1.0
  - @pnpm/store.controller-types@1101.2.0
  - @pnpm/types@1102.1.0
