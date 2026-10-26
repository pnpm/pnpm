## 1101.1.12

### Patch Changes

- `pnpm install` now recreates `node_modules` after a `storeDir` or `virtualStoreDir` change. It used to report "Already up to date".

- A repeat `pnpm install` now renames the virtual store directories after `virtualStoreDirMaxLength` changes. It now runs the builds it skipped before after `dangerouslyAllowAllBuilds` is turned on. It now verifies the lockfile after `trustLockfile` is turned off. Before, it reported "Already up to date" in each of these cases.

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.19
  - @pnpm/building.commands@1101.2.12
  - @pnpm/cli.utils@1101.0.34
  - @pnpm/config.reader@1102.3.7
  - @pnpm/config.version-policy@1100.2.10
  - @pnpm/core-loggers@1101.0.4
  - @pnpm/deps.status@1100.1.32
  - @pnpm/engine.runtime.commands@1101.1.11
  - @pnpm/engine.runtime.system-version@1100.0.15
  - @pnpm/exec.lifecycle@1100.1.26
  - @pnpm/exec.npm-lifecycle@1100.0.7
  - @pnpm/exec.pnpm-cli-runner@1100.0.7
  - @pnpm/installing.client@1100.3.18
  - @pnpm/installing.commands@1101.4.8
  - @pnpm/pkg-manifest.reader@1100.0.24
  - @pnpm/types@1102.1.3
  - @pnpm/workspace.injected-deps-syncer@1100.0.46
  - @pnpm/workspace.project-manifest-reader@1100.1.5
  - @pnpm/workspace.projects-filter@1100.0.51
  - @pnpm/workspace.projects-reader@1101.1.6
  - @pnpm/workspace.projects-sorter@1101.0.3
  - @pnpm/workspace.task-scheduler@1100.0.6
