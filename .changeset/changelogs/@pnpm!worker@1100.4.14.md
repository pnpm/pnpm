## 1100.4.14

### Patch Changes

- `pnpm install` now builds a git dependency with install scripts that an older pnpm release marked in a shared store as not requiring a build [pnpm/tasks#189](https://github.com/pnpm/tasks/issues/189).

- Fixed `pnpm store status` reporting approved builds of git dependencies as modified when the store contains legacy build metadata.

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- Packages that an older pnpm version added to the store now run their install scripts. Before, pnpm could skip those scripts and hardlink the package's files to the store.

- Updated dependencies:
  - @pnpm/fs.graceful-fs@1100.2.6
  - @pnpm/fs.hard-link-dir@1100.0.10
  - @pnpm/store.cafs@1100.3.10
  - @pnpm/store.cafs-types@1100.1.2
  - @pnpm/store.create-cafs-store@1100.0.40
  - @pnpm/store.index@1101.0.4
