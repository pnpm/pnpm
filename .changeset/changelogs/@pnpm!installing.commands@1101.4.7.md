## 1101.4.7

### Patch Changes

- Fixed `pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` failing inside a directory created by `pnpm deploy`. `pnpm deploy` now writes self-contained workspace and lockfile settings. `pnpm deploy` also prunes unused `patchedDependencies` and preserves workspace-level dependency resolutions on deployed directory dependencies.

- Registry revisions are no longer supported. pnpm now fails with `ERR_PNPM_REGISTRY_REVISIONS_UNSUPPORTED` on registry metadata that selects a revision, on a `+rN` version specifier, and on a lockfile entry with a `revision` field. `pnpm update --patches` is removed.

- pnpm now recognizes `externalDependencies` and `autoInstallPeersFromHighestMatch` in `pnpm-workspace.yaml` without warning and forwards both settings to `pnpm install`. Previously, setting `externalDependencies` in `pnpm-workspace.yaml` with `nodeLinker: hoisted` crashed `pnpm install` with `externalDependencies.has is not a function` [pnpm/tasks#166](https://github.com/pnpm/tasks/issues/166).

- With `sharedWorkspaceLockfile: false`, `pnpm install` now runs the `preResolution` hooks and custom resolvers and fetchers of the workspace root's pnpmfile for every project, next to the project's own. A global pnpmfile now runs once per project, and each project's lockfile records the checksum of the root pnpmfile when the project has none of its own. Before, a project's own pnpmfile replaced these root hooks, the global hooks ran twice, and edits to the root pnpmfile could go unnoticed.

- When `pnpm install` resolves a dependency again, it now reuses a version that the install already resolved for the same package, as a fresh install does. Before, it kept the version the lockfile pinned and added a second copy of the package. A locked version still stays when nothing else in the install prefers another version. To pin a transitive dependency on purpose, use `overrides` [#17084](https://github.com/pnpm/pnpm/issues/17084).

- Fixed patching in workspaces with `sharedWorkspaceLockfile: false`. `pnpm patch`, `pnpm patch-commit`, and `pnpm patch-remove` now resolve patch files and `patchedDependencies` entries at the workspace root. `pnpm install` now checks configured patches across all workspace projects before reporting unused patches.

- `minimumReleaseAgeExcludePrune` and `trustPolicyExcludePrune` now keep exclusions for resolved configuration dependencies and their transitive dependencies [pnpm/pnpm#17142](https://github.com/pnpm/pnpm/issues/17142).

- `pnpm prune` now removes extraneous direct packages from `node_modules` when `package.json` and `pnpm-lock.yaml` are already in sync. `pnpm prune` also accepts `--filter` and `--recursive` (`-r`) to prune selected workspace projects without removing dependencies of unselected projects.

- `pnpm install` now removes config dependencies that are no longer declared, including when the entire `configDependencies` setting is removed. Frozen installs reject a lockfile that still records removed config dependencies.

- `pnpm run`, `pnpm exec`, and `pnpm restart` now pass the install flags they were given, such as `--lockfile-dir`, `--modules-dir`, `--store-dir`, `--offline`, and `--ignore-scripts`, to the `pnpm install` that `verifyDepsBeforeRun: install` starts. The check that runs before a script now holds the workspace projects the script's project depends on. It agrees with the repeat-install check on which projects a filtered install left uninstalled, including with per-project lockfiles. Workspace dependencies declared with `link:` are now included in the workspace project graph, so `--filter=<project>...` selects them and, with `sharedWorkspaceLockfile: false` and `injectWorkspacePackages: true`, the check before a script still requires them to be installed. A filtered install no longer forgets the ignored build scripts of the projects it did not select.

- The `verifyDepsBeforeRun` check now notices an edited manifest, lockfile, or patch file even when the edit kept the file's modification time, as `cp -p`, `rsync -a`, or an archive extraction leave it.

  After `pnpm install --filter`, `pnpm run` and `pnpm exec` now notice a changed manifest in a project that install left out. A filtered command for a project that install did not reinstall after a lockfile change now installs first.

  `pnpm dedupe --check` and `pnpm install --dry-run` no longer record the dependencies as installed, so the `verifyDepsBeforeRun` check that follows them still notices outdated dependencies.

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.12
  - @pnpm/building.policy@1100.1.8
  - @pnpm/cli.utils@1101.0.33
  - @pnpm/config.pick-registry-for-package@1101.0.3
  - @pnpm/config.reader@1102.3.6
  - @pnpm/config.version-policy@1100.2.9
  - @pnpm/config.writer@1100.0.34
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/deps.github-actions@1100.1.17
  - @pnpm/deps.inspection.outdated@1100.1.38
  - @pnpm/deps.path@1101.0.7
  - @pnpm/deps.security.signatures@1102.0.10
  - @pnpm/deps.status@1100.1.31
  - @pnpm/global.commands@1102.0.9
  - @pnpm/global.packages@1101.1.8
  - @pnpm/hooks.pnpmfile@1100.0.37
  - @pnpm/installing.context@1101.0.12
  - @pnpm/installing.dedupe.check@1100.1.17
  - @pnpm/installing.deps-installer@1104.2.6
  - @pnpm/installing.env-installer@1103.0.12
  - @pnpm/installing.modules-yaml@1101.0.7
  - @pnpm/lockfile.fs@1100.2.14
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/lockfile.utils@1102.1.7
  - @pnpm/network.auth-header@1101.1.17
  - @pnpm/network.fetch@1100.1.23
  - @pnpm/pkg-manifest.reader@1100.0.23
  - @pnpm/pkg-manifest.utils@1100.4.10
  - @pnpm/resolving.local-resolver@1101.2.7
  - @pnpm/resolving.npm-resolver@1104.2.7
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/store.connection-manager@1101.3.5
  - @pnpm/store.controller@1102.2.6
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.injected-deps-syncer@1100.0.45
  - @pnpm/workspace.project-manifest-reader@1100.1.4
  - @pnpm/workspace.project-manifest-writer@1100.0.21
  - @pnpm/workspace.projects-filter@1100.0.50
  - @pnpm/workspace.projects-graph@1100.0.45
  - @pnpm/workspace.projects-reader@1101.1.5
  - @pnpm/workspace.projects-sorter@1101.0.2
  - @pnpm/workspace.root-finder@1100.1.4
  - @pnpm/workspace.state@1100.0.51
  - @pnpm/workspace.task-scheduler@1100.0.5
  - @pnpm/workspace.workspace-manifest-reader@1100.3.0
  - @pnpm/workspace.workspace-manifest-writer@1100.2.8
