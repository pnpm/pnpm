## 1100.3.10

### Patch Changes

- Fixed `pnpm store status` reporting approved builds of git dependencies as modified when the store contains legacy build metadata.

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- Updated dependencies:
  - @pnpm/fetching.fetcher-base@1100.2.15
  - @pnpm/fs.graceful-fs@1100.2.6
  - @pnpm/store.controller-types@1101.3.6
  - @pnpm/store.file-mode@1100.0.3
