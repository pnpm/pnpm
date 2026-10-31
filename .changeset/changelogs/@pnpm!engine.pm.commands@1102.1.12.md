## 1102.1.12

### Patch Changes

- Commands, `pnpm --version`, `pnpm self-update`, and invocations with unknown flags now apply the same `packageManager` and `devEngines.packageManager` settings when switching to the pinned pnpm version or recording it in `pnpm-lock.yaml`. `--config.<key>` flags and `lockfileDir` now reach the version switch and lockfile sync on every entry point. Unreadable settings in `pnpm-workspace.yaml` or the global `config.yaml` that do not affect the pin decision no longer block the version switch [pnpm/tasks#222](https://github.com/pnpm/tasks/issues/222).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.40
  - @pnpm/building.during-install@1102.2.11
  - @pnpm/config.reader@1102.3.8
  - @pnpm/deps.graph-hasher@1100.3.10
  - @pnpm/global.commands@1102.0.11
  - @pnpm/installing.client@1100.3.19
  - @pnpm/installing.deps-restorer@1103.2.8
  - @pnpm/installing.env-installer@1103.0.14
  - @pnpm/lockfile.fs@1100.2.16
  - @pnpm/store.connection-manager@1101.3.7
  - @pnpm/store.controller@1102.2.8
