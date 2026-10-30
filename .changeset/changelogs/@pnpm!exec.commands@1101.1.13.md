## 1101.1.13

### Patch Changes

- `pnx` and `pnpm create` now run the command in the directory selected by `--dir` or `--workspace-root`. Without either flag, the command still runs in the directory pnpm was started in.

- Commands, `pnpm --version`, `pnpm self-update`, and invocations with unknown flags now apply the same `packageManager` and `devEngines.packageManager` settings when switching to the pinned pnpm version or recording it in `pnpm-lock.yaml`. `--config.<key>` flags and `lockfileDir` now reach the version switch and lockfile sync on every entry point. Unreadable settings in `pnpm-workspace.yaml` or the global `config.yaml` that do not affect the pin decision no longer block the version switch [pnpm/tasks#222](https://github.com/pnpm/tasks/issues/222).

- Updated dependencies:
  - @pnpm/building.commands@1101.2.13
  - @pnpm/config.reader@1102.3.8
  - @pnpm/deps.status@1100.1.33
  - @pnpm/engine.runtime.commands@1101.1.12
  - @pnpm/exec.lifecycle@1100.1.27
  - @pnpm/installing.client@1100.3.19
  - @pnpm/installing.commands@1101.4.9
  - @pnpm/workspace.injected-deps-syncer@1100.0.47
  - @pnpm/workspace.projects-filter@1100.0.52
