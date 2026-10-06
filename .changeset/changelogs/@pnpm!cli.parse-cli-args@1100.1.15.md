## 1100.1.15

### Patch Changes

- `pnpm run` and `pnpm exec` now forward `--config.*` command-line flags to the install started by `verifyDepsBeforeRun` [pnpm/tasks#60](https://github.com/pnpm/tasks/issues/60).

- `pnpm config get` and `pnpm config list` with `--global` or `--location=global` now show only the global configuration. Both flags included the project's `.npmrc` before. `--location=global` also included the project's `pnpm-workspace.yaml`. `pnpm config get --global` failed when the global bin directory was not in PATH [#16598](https://github.com/pnpm/pnpm/issues/16598).

- Updated dependencies:
  - @pnpm/error@1100.2.2
  - @pnpm/workspace.root-finder@1100.1.2
