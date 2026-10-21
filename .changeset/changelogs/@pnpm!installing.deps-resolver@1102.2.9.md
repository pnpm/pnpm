## 1102.2.9

### Patch Changes

- When another workspace project adds a different version of a package, a project's auto-installed peer on that package now moves to the version a fresh install picks. It used to stay on its locked version while any other project still accepted that version [#17058](https://github.com/pnpm/pnpm/issues/17058).

- A package that lists the same dependency in `dependencies` and as an optional peer now gets the peer bound when a project adds a provider for it later. The lockfile now records every peer a package declares, including one its own dependency supplies [#17056](https://github.com/pnpm/pnpm/issues/17056).

- `pnpm deploy` now records the peer dependencies of the workspace packages it copies into the deployed lockfile. Before, pnpm rejected that lockfile with `--frozen-lockfile` when a deployed workspace package declared a peer and `injectWorkspacePackages` was off [#17093](https://github.com/pnpm/pnpm/issues/17093).

- Fixed `pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` failing inside a directory created by `pnpm deploy`. `pnpm deploy` now writes self-contained workspace and lockfile settings. `pnpm deploy` also prunes unused `patchedDependencies` and preserves workspace-level dependency resolutions on deployed directory dependencies.

- Registry revisions are no longer supported. pnpm now fails with `ERR_PNPM_REGISTRY_REVISIONS_UNSUPPORTED` on registry metadata that selects a revision, on a `+rN` version specifier, and on a lockfile entry with a `revision` field. `pnpm update --patches` is removed.

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the dependencies of an injected workspace package no longer match the lockfile. Before, it installed the stale dependencies when `sharedWorkspaceLockfile` was `false` [#17008](https://github.com/pnpm/pnpm/issues/17008).

- An injected workspace dependency now becomes a link when its dependencies match the workspace package's own, also on an install that leaves the package's installed dependencies as they are. Before, such an install kept it as a copy [#17094](https://github.com/pnpm/pnpm/issues/17094).

- An injected workspace package that was linked now becomes a copy when the project that depends on it starts providing one of its peers at a version the package's own workspace context lacks. The link used to stay until that dependency was updated, so the package could not find the peer the project provides [#17057](https://github.com/pnpm/pnpm/issues/17057).

- When `pnpm install` resolves a dependency again, it now reuses a version that the install already resolved for the same package, as a fresh install does. Before, it kept the version the lockfile pinned and added a second copy of the package. A locked version still stays when nothing else in the install prefers another version. To pin a transitive dependency on purpose, use `overrides` [#17084](https://github.com/pnpm/pnpm/issues/17084).

- An optional peer that pnpm adds to a project now keeps the version the lockfile recorded for that project only while the dependency graph still holds it. Otherwise it follows its provider to a version the graph holds. Before, pnpm kept a higher version that only the lockfile held, which left two copies of the peer [#16443](https://github.com/pnpm/pnpm/issues/16443).

- Fixed patching in workspaces with `sharedWorkspaceLockfile: false`. `pnpm patch`, `pnpm patch-commit`, and `pnpm patch-remove` now resolve patch files and `patchedDependencies` entries at the workspace root. `pnpm install` now checks configured patches across all workspace projects before reporting unused patches.

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.4
  - @pnpm/config.version-policy@1100.2.9
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/deps.graph-hasher@1100.3.8
  - @pnpm/deps.path@1101.0.7
  - @pnpm/fetching.pick-fetcher@1100.1.15
  - @pnpm/fs.symlink-dependency@1100.0.25
  - @pnpm/hooks.types@1101.0.6
  - @pnpm/lockfile.preferred-versions@1100.0.39
  - @pnpm/lockfile.pruner@1100.0.28
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/lockfile.utils@1102.1.7
  - @pnpm/patching.config@1100.1.10
  - @pnpm/pkg-manifest.reader@1100.0.23
  - @pnpm/pkg-manifest.utils@1100.4.10
  - @pnpm/resolving.npm-resolver@1104.2.7
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/store.controller-types@1101.3.4
  - @pnpm/types@1102.1.2
