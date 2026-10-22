## 1104.2.6

### Patch Changes

- A package that lists the same dependency in `dependencies` and as an optional peer now gets the peer bound when a project adds a provider for it later. The lockfile now records every peer a package declares, including one its own dependency supplies [#17056](https://github.com/pnpm/pnpm/issues/17056).

- Registry revisions are no longer supported. pnpm now fails with `ERR_PNPM_REGISTRY_REVISIONS_UNSUPPORTED` on registry metadata that selects a revision, on a `+rN` version specifier, and on a lockfile entry with a `revision` field. `pnpm update --patches` is removed.

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the dependencies of an injected workspace package no longer match the lockfile. Before, it installed the stale dependencies when `sharedWorkspaceLockfile` was `false` [#17008](https://github.com/pnpm/pnpm/issues/17008).

- `pnpm add` no longer warns that a gist dependency was replaced from a different source when the new specifier names the same gist in another form, such as `gist:<id>` and `https://gist.github.com/<user>/<id>`.

- A filtered `pnpm install --no-lockfile` with `hoistWorkspacePackages` now hoists every workspace project into `node_modules/.pnpm/node_modules`. It used to hoist only the selected projects [pnpm/tasks#168](https://github.com/pnpm/tasks/issues/168).

- An injected workspace package now keeps only the files its manifest lists after a lifecycle script or a script named in `syncInjectedDepsAfterScripts` rebuilds it. Before, these refreshes copied every file of the package directory into the injected copy, including files that `files` or `.npmignore` leave out. With `deployAllFiles: true` the copy still gets every file.

- When `pnpm install` resolves a dependency again, it now reuses a version that the install already resolved for the same package, as a fresh install does. Before, it kept the version the lockfile pinned and added a second copy of the package. A locked version still stays when nothing else in the install prefers another version. To pin a transitive dependency on purpose, use `overrides` [#17084](https://github.com/pnpm/pnpm/issues/17084).

- Fixed patching in workspaces with `sharedWorkspaceLockfile: false`. `pnpm patch`, `pnpm patch-commit`, and `pnpm patch-remove` now resolve patch files and `patchedDependencies` entries at the workspace root. `pnpm install` now checks configured patches across all workspace projects before reporting unused patches.

- `pnpm prune` now removes extraneous direct packages from `node_modules` when `package.json` and `pnpm-lock.yaml` are already in sync. `pnpm prune` also accepts `--filter` and `--recursive` (`-r`) to prune selected workspace projects without removing dependencies of unselected projects.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.38
  - @pnpm/bins.remover@1100.0.29
  - @pnpm/building.after-install@1103.0.12
  - @pnpm/building.during-install@1102.2.9
  - @pnpm/building.policy@1100.1.8
  - @pnpm/config.normalize-registries@1101.0.4
  - @pnpm/config.package-is-installable@1100.2.4
  - @pnpm/config.version-policy@1100.2.9
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/deps.graph-hasher@1100.3.8
  - @pnpm/deps.path@1101.0.7
  - @pnpm/exec.lifecycle@1100.1.25
  - @pnpm/fs.symlink-dependency@1100.0.25
  - @pnpm/hooks.read-package-hook@1100.3.9
  - @pnpm/hooks.types@1101.0.6
  - @pnpm/installing.context@1101.0.12
  - @pnpm/installing.deps-resolver@1102.2.9
  - @pnpm/installing.deps-restorer@1103.2.6
  - @pnpm/installing.linking.direct-dep-linker@1100.0.25
  - @pnpm/installing.linking.hoist@1100.0.39
  - @pnpm/installing.linking.modules-cleaner@1100.1.30
  - @pnpm/installing.modules-yaml@1101.0.7
  - @pnpm/installing.package-requester@1102.2.6
  - @pnpm/lockfile.filtering@1100.2.13
  - @pnpm/lockfile.fs@1100.2.14
  - @pnpm/lockfile.preferred-versions@1100.0.39
  - @pnpm/lockfile.pruner@1100.0.28
  - @pnpm/lockfile.settings-checker@1100.2.14
  - @pnpm/lockfile.to-pnp@1101.0.11
  - @pnpm/lockfile.utils@1102.1.7
  - @pnpm/lockfile.verification@1100.1.13
  - @pnpm/lockfile.walker@1100.0.28
  - @pnpm/network.auth-header@1101.1.17
  - @pnpm/patching.config@1100.1.10
  - @pnpm/pkg-manifest.utils@1100.4.10
  - @pnpm/pnpr.client@3.1.4
  - @pnpm/resolving.local-resolver@1101.2.7
  - @pnpm/resolving.npm-resolver@1104.2.7
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/store.controller-types@1101.3.4
  - @pnpm/store.index@1101.0.3
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.project-manifest-reader@1100.1.4
