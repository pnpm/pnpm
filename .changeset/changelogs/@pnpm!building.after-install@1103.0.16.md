## 1103.0.16

### Patch Changes

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- Bin shims in the global virtual store no longer include the hoisted modules directory of the project that installed them [#18035](https://github.com/pnpm/pnpm/issues/18035).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.42
  - @pnpm/building.during-install@1102.2.13
  - @pnpm/building.policy@1100.1.10
  - @pnpm/config.reader@1102.3.9
  - @pnpm/deps.graph-builder@1101.1.10
  - @pnpm/deps.graph-hasher@1100.3.12
  - @pnpm/deps.path@1101.0.9
  - @pnpm/exec.lifecycle@1100.1.29
  - @pnpm/installing.context@1101.0.16
  - @pnpm/installing.deps-restorer@1103.2.10
  - @pnpm/installing.modules-yaml@1101.0.9
  - @pnpm/lockfile.settings-checker@1100.2.18
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/lockfile.walker@1100.0.30
  - @pnpm/patching.config@1100.1.12
  - @pnpm/store.cafs@1100.3.10
  - @pnpm/store.connection-manager@1101.3.9
  - @pnpm/store.controller-types@1101.3.6
  - @pnpm/store.index@1101.0.4
