## 1101.1.10

### Patch Changes

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.6
  - @pnpm/deps.graph-hasher@1100.3.12
  - @pnpm/deps.path@1101.0.9
  - @pnpm/hooks.types@1101.0.8
  - @pnpm/installing.modules-yaml@1101.0.9
  - @pnpm/lockfile.fs@1100.2.18
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/patching.config@1100.1.12
  - @pnpm/store.controller-types@1101.3.6
