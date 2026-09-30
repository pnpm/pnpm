## 1101.4.3

### Patch Changes

- `pnpm audit --fix` now updates vulnerable packages in a single project that sets `updateConfig.ignoreDependencies`. It used to leave them on the vulnerable version.

- `pnpm --filter <project> update <pkg>` now fails with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` when the selected projects do not depend on `<pkg>`, also in a workspace with a shared lockfile and a root project. It used to install and exit successfully.

- `pnpm import` no longer crashes when the imported lockfile or a Yarn patch refers to a package named `constructor`.

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.8
  - @pnpm/config.reader@1102.3.2
  - @pnpm/config.writer@1100.0.30
  - @pnpm/deps.github-actions@1100.1.13
  - @pnpm/deps.inspection.outdated@1100.1.34
  - @pnpm/deps.security.signatures@1102.0.7
  - @pnpm/deps.status@1100.1.27
  - @pnpm/global.commands@1102.0.5
  - @pnpm/global.packages@1101.1.6
  - @pnpm/installing.context@1101.0.8
  - @pnpm/installing.deps-installer@1104.2.2
  - @pnpm/installing.env-installer@1103.0.8
  - @pnpm/lockfile.fs@1100.2.11
  - @pnpm/network.fetch@1100.1.20
  - @pnpm/resolving.npm-resolver@1104.2.3
  - @pnpm/store.connection-manager@1101.3.1
  - @pnpm/store.controller@1102.2.2
  - @pnpm/workspace.injected-deps-syncer@1100.0.41
  - @pnpm/workspace.projects-filter@1100.0.46
  - @pnpm/workspace.projects-graph@1100.0.41
  - @pnpm/workspace.state@1100.0.47
  - @pnpm/workspace.workspace-manifest-writer@1100.2.4
