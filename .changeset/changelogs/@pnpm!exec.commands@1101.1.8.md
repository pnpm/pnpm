## 1101.1.8

### Patch Changes

- `pnpm run` and `pnpm exec` now warn and run the command when the install that `verifyDepsBeforeRun` starts fails. This lets scripts run in sandboxes where pnpm cannot install, such as containers with a read-only store or no network [#15173](https://github.com/pnpm/pnpm/issues/15173).

- A filtered `pnpm run` or `pnpm exec` now finds dependencies out of date when a workspace dependency of a selected project has no `node_modules` directory, as after a filtered install. With `verifyDepsBeforeRun: install`, pnpm installs that dependency before running the command ([pnpm/tasks#45](https://github.com/pnpm/tasks/issues/45)).

- Updated dependencies:
  - @pnpm/building.commands@1101.2.8
  - @pnpm/cli.utils@1101.0.31
  - @pnpm/config.reader@1102.3.3
  - @pnpm/config.version-policy@1100.2.6
  - @pnpm/core-loggers@1101.0.2
  - @pnpm/deps.status@1100.1.28
  - @pnpm/engine.runtime.commands@1101.1.7
  - @pnpm/engine.runtime.system-version@1100.0.13
  - @pnpm/exec.esm-node-path-loader@1100.0.1
  - @pnpm/exec.lifecycle@1100.1.22
  - @pnpm/exec.npm-lifecycle@1100.0.4
  - @pnpm/exec.pnpm-cli-runner@1100.0.5
  - @pnpm/installing.client@1100.3.14
  - @pnpm/installing.commands@1101.4.4
  - @pnpm/store.path@1100.1.1
  - @pnpm/workspace.injected-deps-syncer@1100.0.42
  - @pnpm/workspace.project-manifest-reader@1100.1.2
  - @pnpm/workspace.projects-filter@1100.0.47
  - @pnpm/workspace.projects-reader@1101.1.2
