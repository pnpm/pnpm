## 1100.1.19

### Patch Changes

- Commands, `pnpm --version`, `pnpm self-update`, and invocations with unknown flags now apply the same `packageManager` and `devEngines.packageManager` settings when switching to the pinned pnpm version or recording it in `pnpm-lock.yaml`. `--config.<key>` flags and `lockfileDir` now reach the version switch and lockfile sync on every entry point. Unreadable settings in `pnpm-workspace.yaml` or the global `config.yaml` that do not affect the pin decision no longer block the version switch [pnpm/tasks#222](https://github.com/pnpm/tasks/issues/222).

- Updated dependencies:
  - @pnpm/workspace.root-finder@1100.1.6
