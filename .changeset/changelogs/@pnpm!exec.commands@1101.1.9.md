## 1101.1.9

### Patch Changes

- `pnpm dlx` with `--package` but no command now reports a clear error: `'pnpm dlx' requires a command to run`. Previously it installed the package and then crashed trying to run an empty command.

- `pnpm run` and `pnpm exec` now forward `--config.*` command-line flags to the install started by `verifyDepsBeforeRun` [pnpm/tasks#60](https://github.com/pnpm/tasks/issues/60).

- Updated dependencies:
  - @pnpm/building.commands@1101.2.9
  - @pnpm/catalogs.resolver@1100.1.2
  - @pnpm/cli.utils@1101.0.32
  - @pnpm/config.reader@1102.3.4
  - @pnpm/config.version-policy@1100.2.7
  - @pnpm/crypto.hash@1100.0.8
  - @pnpm/deps.status@1100.1.29
  - @pnpm/engine.runtime.commands@1101.1.8
  - @pnpm/error@1100.2.2
  - @pnpm/exec.lifecycle@1100.1.23
  - @pnpm/exec.npm-lifecycle@1100.0.5
  - @pnpm/fs.dir-lock@1100.0.1
  - @pnpm/installing.client@1100.3.15
  - @pnpm/installing.commands@1101.4.5
  - @pnpm/pkg-manifest.reader@1100.0.22
  - @pnpm/store.path@1100.1.2
  - @pnpm/workspace.injected-deps-syncer@1100.0.43
  - @pnpm/workspace.project-manifest-reader@1100.1.3
  - @pnpm/workspace.projects-filter@1100.0.48
  - @pnpm/workspace.projects-reader@1101.1.3
  - @pnpm/workspace.task-scheduler@1100.0.4
