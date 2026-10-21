## 1101.0.12

### Patch Changes

- `pnpm prune` now removes extraneous direct packages from `node_modules` when `package.json` and `pnpm-lock.yaml` are already in sync. `pnpm prune` also accepts `--filter` and `--recursive` (`-r`) to prune selected workspace projects without removing dependencies of unselected projects.

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/installing.modules-yaml@1101.0.7
  - @pnpm/installing.read-projects-context@1101.0.11
  - @pnpm/lockfile.fs@1100.2.14
  - @pnpm/lockfile.pruner@1100.0.28
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/store.controller@1102.2.6
  - @pnpm/types@1102.1.2
