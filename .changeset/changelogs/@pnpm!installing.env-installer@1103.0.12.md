## 1103.0.12

### Patch Changes

- `minimumReleaseAgeExcludePrune` and `trustPolicyExcludePrune` now keep exclusions for resolved configuration dependencies and their transitive dependencies [pnpm/pnpm#17142](https://github.com/pnpm/pnpm/issues/17142).

- `pnpm install` now removes config dependencies that are no longer declared, including when the entire `configDependencies` setting is removed. Frozen installs reject a lockfile that still records removed config dependencies.

- pnpm now applies `trustPolicy` to the pnpm version a project pins in `packageManager` or `devEngines.packageManager`. A pinned version whose trust evidence was downgraded fails the command before pnpm switches to it or records it in the lockfile [#16319](https://github.com/pnpm/pnpm/issues/16319).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.4
  - @pnpm/config.pick-registry-for-package@1101.0.3
  - @pnpm/config.version-policy@1100.2.9
  - @pnpm/config.writer@1100.0.34
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/deps.graph-hasher@1100.3.8
  - @pnpm/deps.path@1101.0.7
  - @pnpm/fs.symlink-dependency@1100.0.25
  - @pnpm/installing.deps-resolver@1102.2.9
  - @pnpm/lockfile.fs@1100.2.14
  - @pnpm/lockfile.pruner@1100.0.28
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/lockfile.utils@1102.1.7
  - @pnpm/network.auth-header@1101.1.17
  - @pnpm/network.fetch@1100.1.23
  - @pnpm/pkg-manifest.reader@1100.0.23
  - @pnpm/resolving.npm-resolver@1104.2.7
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/resolving.tarball-url@1101.1.5
  - @pnpm/store.controller@1102.2.6
  - @pnpm/store.controller-types@1101.3.4
  - @pnpm/types@1102.1.2
