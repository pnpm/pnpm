## 1100.1.33

### Patch Changes

- `pnpm install` now relinks a `workspace:` or `link:` dependency whose link was deleted from `node_modules`. It used to report "Already up to date" and leave the link missing [#17485](https://github.com/pnpm/pnpm/issues/17485).

- `pnpm install` now detects a change to a local tarball or directory that only a pnpmfile `readPackage` hook depends on [pnpm/tasks#227](https://github.com/pnpm/tasks/issues/227). A repeat install reported "Already up to date" and kept the old contents before.

- Updated dependencies:
  - @pnpm/config.reader@1102.3.8
  - @pnpm/installing.context@1101.0.14
  - @pnpm/lockfile.fs@1100.2.16
  - @pnpm/lockfile.settings-checker@1100.2.16
  - @pnpm/lockfile.utils@1102.1.9
  - @pnpm/lockfile.verification@1100.1.15
  - @pnpm/workspace.state@1100.0.53
  - @pnpm/workspace.workspace-manifest-reader@1100.3.2
