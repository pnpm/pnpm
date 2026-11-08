## 1101.4.10

### Patch Changes

- Packages from the default registry keep plain lockfile keys, even when a prefix such as `npmjs` points at the registry set as `registry`. A dependency written with such a prefix shares the plain entry of the same package. The next install rewrites keys that earlier versions qualified this way [#17575](https://github.com/pnpm/pnpm/issues/17575).

- In a workspace, `pnpm link <dir>` now adds the `link:` dependency to the `package.json` of the project where the command runs, not to the workspace root's `package.json`. The link is created in that project's `node_modules`, as documented. The override is still written to the root `pnpm-workspace.yaml`.

- `pnpm link` with several paths now records every linked package in the `overrides` of `pnpm-workspace.yaml`. Some of them could be missing before.

- `pnpm link` now fails without changing `package.json` or `pnpm-workspace.yaml` when the linked package's name is not a valid npm package name. It used to write that name to both files, which made every later install fail [#17475](https://github.com/pnpm/pnpm/issues/17475).

- `pnpm update` with only negated selectors, such as `pnpm update '!foo'`, now leaves the dependencies listed in `update.ignoreDeps` unchanged [pnpm/tasks#243](https://github.com/pnpm/tasks/issues/243).

- `pnpm update --depth 0` without selectors now does nothing when every direct dependency is listed in `update.ignoreDeps`. Before, it failed with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` [pnpm/tasks#251](https://github.com/pnpm/tasks/issues/251).

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.15
  - @pnpm/config.pick-registry-for-package@1101.0.5
  - @pnpm/deps.inspection.outdated@1100.1.41
  - @pnpm/deps.status@1100.1.34
  - @pnpm/global.commands@1102.0.12
  - @pnpm/global.packages@1101.1.10
  - @pnpm/installing.context@1101.0.15
  - @pnpm/installing.deps-installer@1104.2.9
  - @pnpm/installing.deps-resolver@1102.2.12
  - @pnpm/installing.env-installer@1103.0.15
  - @pnpm/lockfile.fs@1100.2.17
  - @pnpm/lockfile.utils@1102.1.10
  - @pnpm/resolving.npm-resolver@1104.2.9
  - @pnpm/store.connection-manager@1101.3.8
  - @pnpm/store.controller@1102.2.9
  - @pnpm/workspace.injected-deps-syncer@1100.0.48
  - @pnpm/workspace.projects-filter@1100.0.53
  - @pnpm/workspace.projects-graph@1100.0.47
  - @pnpm/workspace.projects-sorter@1101.0.4
  - @pnpm/workspace.state@1100.0.54
  - @pnpm/workspace.task-scheduler@1100.0.7
