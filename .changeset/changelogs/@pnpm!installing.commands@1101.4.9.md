## 1101.4.9

### Patch Changes

- `pnpm install` now detects a change to a local tarball or directory that only a pnpmfile `readPackage` hook depends on [pnpm/tasks#227](https://github.com/pnpm/tasks/issues/227). A repeat install reported "Already up to date" and kept the old contents before.

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.14
  - @pnpm/config.reader@1102.3.8
  - @pnpm/config.writer@1100.0.36
  - @pnpm/deps.inspection.outdated@1100.1.40
  - @pnpm/deps.status@1100.1.33
  - @pnpm/global.commands@1102.0.11
  - @pnpm/installing.context@1101.0.14
  - @pnpm/installing.deps-installer@1104.2.8
  - @pnpm/installing.env-installer@1103.0.14
  - @pnpm/lockfile.fs@1100.2.16
  - @pnpm/lockfile.utils@1102.1.9
  - @pnpm/store.connection-manager@1101.3.7
  - @pnpm/store.controller@1102.2.8
  - @pnpm/workspace.injected-deps-syncer@1100.0.47
  - @pnpm/workspace.projects-filter@1100.0.52
  - @pnpm/workspace.root-finder@1100.1.6
  - @pnpm/workspace.state@1100.0.53
  - @pnpm/workspace.workspace-manifest-reader@1100.3.2
  - @pnpm/workspace.workspace-manifest-writer@1100.2.10
