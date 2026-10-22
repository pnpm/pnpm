## 1103.2.6

### Patch Changes

- Filtered installs remove obsolete dependencies from the installed lockfile while preserving dependencies still needed by unselected projects.

- An injected workspace package now keeps only the files its manifest lists after a lifecycle script or a script named in `syncInjectedDepsAfterScripts` rebuilds it. Before, these refreshes copied every file of the package directory into the injected copy, including files that `files` or `.npmignore` leave out. With `deployAllFiles: true` the copy still gets every file.

- `verifyDepsBeforeRun` no longer treats the workspace root as up to date after a filtered install with `pruneDirectDependencies: true` skipped it [pnpm/tasks#181](https://github.com/pnpm/tasks/issues/181).

- `pnpm prune` now removes extraneous direct packages from `node_modules` when `package.json` and `pnpm-lock.yaml` are already in sync. `pnpm prune` also accepts `--filter` and `--recursive` (`-r`) to prune selected workspace projects without removing dependencies of unselected projects.

- `pnpm run`, `pnpm exec`, and `pnpm restart` now pass the install flags they were given, such as `--lockfile-dir`, `--modules-dir`, `--store-dir`, `--offline`, and `--ignore-scripts`, to the `pnpm install` that `verifyDepsBeforeRun: install` starts. The check that runs before a script now holds the workspace projects the script's project depends on. It agrees with the repeat-install check on which projects a filtered install left uninstalled, including with per-project lockfiles. Workspace dependencies declared with `link:` are now included in the workspace project graph, so `--filter=<project>...` selects them and, with `sharedWorkspaceLockfile: false` and `injectWorkspacePackages: true`, the check before a script still requires them to be installed. A filtered install no longer forgets the ignored build scripts of the projects it did not select.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.38
  - @pnpm/bins.remover@1100.0.29
  - @pnpm/building.during-install@1102.2.9
  - @pnpm/building.policy@1100.1.8
  - @pnpm/config.normalize-registries@1101.0.4
  - @pnpm/config.package-is-installable@1100.2.4
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/deps.graph-builder@1101.1.6
  - @pnpm/deps.graph-hasher@1100.3.8
  - @pnpm/deps.path@1101.0.7
  - @pnpm/exec.lifecycle@1100.1.25
  - @pnpm/fs.symlink-dependency@1100.0.25
  - @pnpm/installing.linking.direct-dep-linker@1100.0.25
  - @pnpm/installing.linking.hoist@1100.0.39
  - @pnpm/installing.linking.modules-cleaner@1100.1.30
  - @pnpm/installing.linking.real-hoist@1100.1.23
  - @pnpm/installing.modules-yaml@1101.0.7
  - @pnpm/installing.package-requester@1102.2.6
  - @pnpm/lockfile.filtering@1100.2.13
  - @pnpm/lockfile.fs@1100.2.14
  - @pnpm/lockfile.to-pnp@1101.0.11
  - @pnpm/lockfile.utils@1102.1.7
  - @pnpm/patching.config@1100.1.10
  - @pnpm/pkg-manifest.reader@1100.0.23
  - @pnpm/pnpr.client@3.1.4
  - @pnpm/store.controller-types@1101.3.4
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.project-manifest-reader@1100.1.4
