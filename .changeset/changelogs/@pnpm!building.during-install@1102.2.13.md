## 1102.2.13

### Patch Changes

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- Bin shims in the global virtual store no longer include the hoisted modules directory of the project that installed them [#18035](https://github.com/pnpm/pnpm/issues/18035).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.42
  - @pnpm/config.package-is-installable@1100.2.6
  - @pnpm/config.reader@1102.3.9
  - @pnpm/deps.graph-hasher@1100.3.12
  - @pnpm/deps.path@1101.0.9
  - @pnpm/exec.lifecycle@1100.1.29
  - @pnpm/fs.graceful-fs@1100.2.6
  - @pnpm/fs.hard-link-dir@1100.0.10
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/pnpr.client@3.1.8
  - @pnpm/store.controller-types@1101.3.6
