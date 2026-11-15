## 1103.2.10

### Patch Changes

- With `dedupeDirectDeps`, `pnpm install` now compares `link:` dependencies by the directory they point at. A project's `link:` dependency is removed when the workspace root links the same directory under a different path. It is kept when the root's `link:` dependency has the same text but points at another directory.

  With `nodeLinker: hoisted` and `dedupeDirectDeps`, `pnpm install` now removes a project's `link:` dependency that the workspace root provides.

  With `nodeLinker: hoisted`, `pnpm install` now reports each `link:` dependency it removes.

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- `pnpm fetch` no longer fails with `ENOENT` when `nodeLinker` is `hoisted` and the project configures `patchedDependencies` or `allowBuilds`. `pnpm fetch` also preserves an existing hoisted `node_modules` directory [#18037](https://github.com/pnpm/pnpm/issues/18037).

- With `nodeLinker: hoisted`, `pnpm install --filter` in a sparse checkout now drops previously installed workspace projects that are missing from disk from the hoisted `node_modules` tree. It no longer recreates `node_modules` inside a missing project's directory. Packages that only the missing project used are also removed from the root `node_modules`.

- With `nodeLinker: hoisted` and a custom `modulesDir`, pnpm now installs a workspace project's dependencies that cannot be hoisted to the root into the project's `modulesDir`. The next install that relinks the project removes the copies that earlier installs left in its `node_modules`. A later install restores the project's `modulesDir` when it was removed. Package directories that pnpm did not install are now also moved to `.ignored` in a custom `modulesDir`.

- pnpm no longer fails to remove a dependency whose `package.json` is malformed. It removes the dependency and keeps its bins [pnpm/tasks#295](https://github.com/pnpm/tasks/issues/295).

- Bin shims in the global virtual store no longer include the hoisted modules directory of the project that installed them [#18035](https://github.com/pnpm/pnpm/issues/18035).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.42
  - @pnpm/bins.remover@1100.0.32
  - @pnpm/building.during-install@1102.2.13
  - @pnpm/building.policy@1100.1.10
  - @pnpm/config.package-is-installable@1100.2.6
  - @pnpm/deps.graph-builder@1101.1.10
  - @pnpm/deps.graph-hasher@1100.3.12
  - @pnpm/deps.path@1101.0.9
  - @pnpm/exec.lifecycle@1100.1.29
  - @pnpm/installing.linking.hoist@1100.0.43
  - @pnpm/installing.linking.modules-cleaner@1100.1.34
  - @pnpm/installing.linking.real-hoist@1100.1.27
  - @pnpm/installing.modules-yaml@1101.0.9
  - @pnpm/installing.package-requester@1102.2.10
  - @pnpm/lockfile.filtering@1100.2.17
  - @pnpm/lockfile.fs@1100.2.18
  - @pnpm/lockfile.to-pnp@1101.0.15
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/patching.config@1100.1.12
  - @pnpm/pnpr.client@3.1.8
  - @pnpm/resolving.npm-resolver@1104.2.10
  - @pnpm/resolving.resolver-base@1101.3.5
  - @pnpm/store.controller-types@1101.3.6
  - @pnpm/workspace.project-manifest-reader@1100.1.6
  - @pnpm/workspace.state@1100.0.55
