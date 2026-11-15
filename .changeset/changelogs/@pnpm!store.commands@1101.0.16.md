## 1101.0.16

### Patch Changes

- Fixed `pnpm store status` reporting approved builds of git dependencies as modified when the store contains legacy build metadata.

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- Updated dependencies:
  - @pnpm/building.policy@1100.1.10
  - @pnpm/cli.utils@1101.0.35
  - @pnpm/config.reader@1102.3.9
  - @pnpm/deps.graph-builder@1101.1.10
  - @pnpm/deps.path@1101.0.9
  - @pnpm/fs.graceful-fs@1100.2.6
  - @pnpm/global.packages@1101.1.11
  - @pnpm/installing.client@1100.3.21
  - @pnpm/installing.context@1101.0.16
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/store.cafs@1100.3.10
  - @pnpm/store.connection-manager@1101.3.9
  - @pnpm/store.controller-types@1101.3.6
  - @pnpm/store.index@1101.0.4
