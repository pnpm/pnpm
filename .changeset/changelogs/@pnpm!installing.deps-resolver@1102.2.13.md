## 1102.2.13

### Patch Changes

- With `dedupePeerDependents: false`, a missing optional peer is no longer resolved from a version that only another workspace project depends on. The workspace root's dependencies still count if `resolvePeersFromWorkspaceRoot` is `true`. Before, a tool in one project's `devDependencies`, such as `webpack-cli`, could become an optional peer of another project's production dependencies [pnpm/pnpm#16666](https://github.com/orgs/pnpm/discussions/16666).

- Fixed aliased dependencies sharing the wrong peer provider across workspace projects when a child uses another version of the aliased package as a peer.

- Dependencies declared as `workspace:` entries in catalogs now remain in the lockfile when `excludeLinksFromLockfile` is enabled. Frozen installs can restore these links after `node_modules` is removed [#260](https://github.com/pnpm/tasks/issues/260).

- Fixed cyclic peer dependencies sharing the wrong provider across workspace projects when the projects select different versions of that provider.

- Conflicting non-semver peer dependencies now remain missing when `autoInstallPeersFromHighestMatch` is enabled. Identical non-semver peer specifiers are preserved. Preferred registry versions no longer replace local paths or protocol-prefixed specifiers when installing missing peers [pnpm/tasks#272](https://github.com/pnpm/tasks/issues/272).

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- `pnpm dedupe` now keeps a locked runtime version, from `devEngines.runtime` or from a dependency's `engines.runtime`, when the declared range still admits it. `pnpm dedupe --check` no longer fails each time a newer Node.js release in the range comes out [#17984](https://github.com/pnpm/pnpm/issues/17984).

- Fixed installation and peer dependency reporting for packages named `constructor` or `toString`. Optional peers with these names are also recognized when declared only in `peerDependenciesMeta`.

- New workspace projects now prefer peer versions already pinned in the lockfile. Dependencies that are also optional peers no longer cause an unrelated project to select an older peer version.

- `pnpm deploy --offline --trust-lockfile` now reuses dependencies pinned in the source lockfile from a populated store when registry metadata is unavailable. This works with per-project lockfiles, `--legacy`, and npm aliases [pnpm/pnpm#17888](https://github.com/pnpm/pnpm/issues/17888).

- `pnpm install` no longer gives a project an optional peer dependency that it skipped because a required peer would conflict with the project's dependencies. Before, `dedupePeerDependents` merged the project's copy of the package that declares the peer with another project's copy that resolves it [#17781](https://github.com/pnpm/pnpm/issues/17781).

- `pnpm install` no longer fills in a missing optional peer dependency if a required peer it brings in conflicts with a version the project depends on. Before, such a peer was installed along with it and reported as unmet [pnpm/pnpm#17718](https://github.com/orgs/pnpm/discussions/17718).

- Fixed repeated installs changing required peer dependency versions after adding a dependency.

- Unchanged installs now preserve peer contexts when peer-dependent packages are deduplicated. Removing a workspace dependency preserves nested peer providers still required by the remaining packages.

  Unchanged installs retain valid locked peer providers even when a direct dependency provides another compatible version. Explicit `pnpm add` and `pnpm update` requests take precedence over those retained bindings.

- Repeated installs now keep automatically installed peer versions stable when workspace projects use different ranges of the same dependency. Adding another ranged provider also preserves a valid peer pin.

- Updated dependencies:
  - @pnpm/deps.graph-hasher@1100.3.12
  - @pnpm/deps.path@1101.0.9
  - @pnpm/fetching.pick-fetcher@1100.1.17
  - @pnpm/fs.graceful-fs@1100.2.6
  - @pnpm/hooks.types@1101.0.8
  - @pnpm/lockfile.preferred-versions@1100.0.43
  - @pnpm/lockfile.pruner@1100.0.30
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/patching.config@1100.1.12
  - @pnpm/pkg-manifest.utils@1100.4.12
  - @pnpm/resolving.npm-resolver@1104.2.10
  - @pnpm/resolving.resolver-base@1101.3.5
  - @pnpm/store.controller-types@1101.3.6
