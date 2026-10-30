## 1100.0.53

### Patch Changes

- `pnpm install` now detects a change to a local tarball or directory that only a pnpmfile `readPackage` hook depends on [pnpm/tasks#227](https://github.com/pnpm/tasks/issues/227). A repeat install reported "Already up to date" and kept the old contents before.

- Updated dependencies:
  - @pnpm/config.reader@1102.3.8
