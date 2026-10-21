## 1100.1.30

### Patch Changes

- `pnpm prune` now removes extraneous direct packages from `node_modules` when `package.json` and `pnpm-lock.yaml` are already in sync. `pnpm prune` also accepts `--filter` and `--recursive` (`-r`) to prune selected workspace projects without removing dependencies of unselected projects.

- Updated dependencies:
  - @pnpm/bins.remover@1100.0.29
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/deps.path@1101.0.7
  - @pnpm/lockfile.filtering@1100.2.13
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/lockfile.utils@1102.1.7
  - @pnpm/store.controller-types@1101.3.4
  - @pnpm/types@1102.1.2
