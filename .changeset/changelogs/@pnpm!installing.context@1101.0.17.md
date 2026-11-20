## 1101.0.17

### Patch Changes

- `pnpm ci` now validates that `pnpm-lock.yaml` exists and matches `package.json` before removing `node_modules`. `pnpm clean` and `pnpm ci` now honor `--filter`, `--filter-prod`, and `--workspace-root`. `pnpm clean` and `pnpm ci` also clean root `node_modules` in workspaces without a root `package.json`. `pnpm clean --lockfile` now removes per-project lockfiles and lockfiles inside a custom `lockfileDir`.

- Fixed `pnpm install --merge-git-branch-lockfiles --frozen-lockfile` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a project sets or removes `publishConfig.directory`, `publishConfig.linkDirectory`, or `dependenciesMeta`. Lockfile merging now preserves and reconciles `publishDirectory`, `linkDirectory`, and `dependenciesMeta` on importers.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.43
  - @pnpm/core-loggers@1101.0.5
  - @pnpm/installing.modules-yaml@1101.0.10
  - @pnpm/installing.read-projects-context@1101.0.16
  - @pnpm/lockfile.fs@1100.2.19
  - @pnpm/lockfile.pruner@1100.0.31
  - @pnpm/resolving.resolver-base@1101.3.6
  - @pnpm/store.controller@1102.2.11
  - @pnpm/types@1102.1.4
