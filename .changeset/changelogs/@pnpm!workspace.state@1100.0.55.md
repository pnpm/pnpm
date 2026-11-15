## 1100.0.55

### Patch Changes

- A repeat `pnpm install` now runs a full install when the `runtimeOnFail` setting changes. Before, it printed "Already up to date" and did not install the runtime a project declares in `devEngines.runtime` [pnpm/tasks#303](https://github.com/pnpm/tasks/issues/303).

- Updated dependencies:
  - @pnpm/config.reader@1102.3.9
  - @pnpm/fs.graceful-fs@1100.2.6
