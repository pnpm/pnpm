## 1102.2.10

### Patch Changes

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- `pnpm store prune` now stops if a dependency link cannot be read. This keeps installed packages from being removed after an incomplete scan.

- Updated dependencies:
  - @pnpm/config.reader@1102.3.9
  - @pnpm/crypto.hash@1100.0.9
  - @pnpm/deps.graph-hasher@1100.3.12
  - @pnpm/fetching.fetcher-base@1100.2.15
  - @pnpm/hooks.types@1101.0.8
  - @pnpm/installing.modules-yaml@1101.0.9
  - @pnpm/installing.package-requester@1102.2.10
  - @pnpm/lockfile.fs@1100.2.18
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/resolving.resolver-base@1101.3.5
  - @pnpm/store.cafs@1100.3.10
  - @pnpm/store.controller-types@1101.3.6
  - @pnpm/store.create-cafs-store@1100.0.40
  - @pnpm/store.index@1101.0.4
  - @pnpm/workspace.project-manifest-reader@1100.1.6
