## 1104.2.10

### Patch Changes

- `pnpm install --frozen-lockfile` now succeeds when a workspace project recorded in `pnpm-lock.yaml` has an empty directory, such as a git submodule that was not checked out [#17712](https://github.com/pnpm/pnpm/issues/17712).

- `pnpm install --filter` in a sparse checkout now counts the dependencies of workspace projects that are missing from disk when it resolves peer dependencies and marks packages as optional. The lockfile it writes no longer differs from a full checkout in those entries [pnpm/pnpm#17791](https://github.com/pnpm/pnpm/issues/17791).

- Programmatic install mutations that omit workspace project manifests now resolve locally when a pnpr server is configured [pnpm/tasks#59](https://github.com/pnpm/tasks/issues/59).

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- `pnpm fetch` no longer fails with `ENOENT` when `nodeLinker` is `hoisted` and the project configures `patchedDependencies` or `allowBuilds`. `pnpm fetch` also preserves an existing hoisted `node_modules` directory [#18037](https://github.com/pnpm/pnpm/issues/18037).

- `pnpm dedupe` now keeps a locked runtime version, from `devEngines.runtime` or from a dependency's `engines.runtime`, when the declared range still admits it. `pnpm dedupe --check` no longer fails each time a newer Node.js release in the range comes out [#17984](https://github.com/pnpm/pnpm/issues/17984).

- Fixed installation and peer dependency reporting for packages named `constructor` or `toString`. Optional peers with these names are also recognized when declared only in `peerDependenciesMeta`.

- New workspace projects now prefer peer versions already pinned in the lockfile. Dependencies that are also optional peers no longer cause an unrelated project to select an older peer version.

- `pnpm deploy --offline --trust-lockfile` now reuses dependencies pinned in the source lockfile from a populated store when registry metadata is unavailable. This works with per-project lockfiles, `--legacy`, and npm aliases [pnpm/pnpm#17888](https://github.com/pnpm/pnpm/issues/17888).

- Fixed `pnpm install` keeping stale peer bindings after dependency changes. Required dependencies no longer remain marked optional when another workspace project removes a shared package [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

- Bin shims in the global virtual store no longer include the hoisted modules directory of the project that installed them [#18035](https://github.com/pnpm/pnpm/issues/18035).

- Version-selector `overrides` entries such as `"ms@^2.0.0": "2.1.1"` now apply to dependencies declared with the `catalog:` protocol when the catalog entry's range matches the selector [#17922](https://github.com/pnpm/pnpm/issues/17922).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.42
  - @pnpm/bins.remover@1100.0.32
  - @pnpm/building.after-install@1103.0.16
  - @pnpm/building.during-install@1102.2.13
  - @pnpm/building.policy@1100.1.10
  - @pnpm/config.package-is-installable@1100.2.6
  - @pnpm/crypto.hash@1100.0.9
  - @pnpm/deps.graph-hasher@1100.3.12
  - @pnpm/deps.path@1101.0.9
  - @pnpm/exec.lifecycle@1100.1.29
  - @pnpm/hooks.read-package-hook@1100.3.12
  - @pnpm/hooks.types@1101.0.8
  - @pnpm/installing.context@1101.0.16
  - @pnpm/installing.deps-resolver@1102.2.13
  - @pnpm/installing.deps-restorer@1103.2.10
  - @pnpm/installing.linking.hoist@1100.0.43
  - @pnpm/installing.linking.modules-cleaner@1100.1.34
  - @pnpm/installing.modules-yaml@1101.0.9
  - @pnpm/installing.package-requester@1102.2.10
  - @pnpm/lockfile.filtering@1100.2.17
  - @pnpm/lockfile.fs@1100.2.18
  - @pnpm/lockfile.preferred-versions@1100.0.43
  - @pnpm/lockfile.pruner@1100.0.30
  - @pnpm/lockfile.settings-checker@1100.2.18
  - @pnpm/lockfile.to-pnp@1101.0.15
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/lockfile.verification@1100.1.17
  - @pnpm/lockfile.walker@1100.0.30
  - @pnpm/patching.config@1100.1.12
  - @pnpm/pkg-manifest.utils@1100.4.12
  - @pnpm/pnpr.client@3.1.8
  - @pnpm/resolving.local-resolver@1101.2.9
  - @pnpm/resolving.npm-resolver@1104.2.10
  - @pnpm/resolving.resolver-base@1101.3.5
  - @pnpm/store.controller-types@1101.3.6
  - @pnpm/store.index@1101.0.4
  - @pnpm/workspace.project-manifest-reader@1100.1.6
