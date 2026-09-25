## 1100.0.33

### Patch Changes

- Workspace projects that `hoistPattern` or `publicHoistPattern` selects are now hoisted on every install. A project added to the workspace was not hoisted until `node_modules` was deleted and reinstalled. A workspace that installs nothing from a registry hoisted none of its projects at all [#3642](https://github.com/pnpm/pnpm/issues/3642).

- Fixed `pnpm install` failing with `EEXIST` when a concurrent install cleared the file or directory that was occupying a symlink path. On Windows, a symlink another process is still holding is no longer moved aside and recreated.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.33
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/fs.symlink-dependency@1100.0.21
  - @pnpm/types@1102.1.1
