## 1101.1.11

### Patch Changes

- An injected workspace package now keeps only the files its manifest lists after a lifecycle script or a script named in `syncInjectedDepsAfterScripts` rebuilds it. Before, these refreshes copied every file of the package directory into the injected copy, including files that `files` or `.npmignore` leave out. With `deployAllFiles: true` the copy still gets every file.

- `pnpm run`, `pnpm exec`, and `pnpm restart` now pass the install flags they were given, such as `--lockfile-dir`, `--modules-dir`, `--store-dir`, `--offline`, and `--ignore-scripts`, to the `pnpm install` that `verifyDepsBeforeRun: install` starts. The check that runs before a script now holds the workspace projects the script's project depends on. It agrees with the repeat-install check on which projects a filtered install left uninstalled, including with per-project lockfiles. Workspace dependencies declared with `link:` are now included in the workspace project graph, so `--filter=<project>...` selects them and, with `sharedWorkspaceLockfile: false` and `injectWorkspacePackages: true`, the check before a script still requires them to be installed. A filtered install no longer forgets the ignored build scripts of the projects it did not select.

- With `sharedWorkspaceLockfile: false`, a script listed in `syncInjectedDepsAfterScripts` now updates the injected copies of the package it ran in. Before, those copies kept the files from the last install.

- A script listed in `syncInjectedDepsAfterScripts` now updates the injected copies of a package that publishes from `publishConfig.directory`. Before, those copies kept the files from the last install.

- `verifyDepsBeforeRun` now checks whether each importer's resolved catalog versions match the catalog snapshot in `pnpm-lock.yaml`.

  `verifyDepsBeforeRun` now respects `lockfileDir` in single-project repositories.

  When the install that runs before a script leaves `node_modules` out of date, pnpm now prints a warning.

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.18
  - @pnpm/building.commands@1101.2.11
  - @pnpm/cli.utils@1101.0.33
  - @pnpm/config.reader@1102.3.6
  - @pnpm/config.version-policy@1100.2.9
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/deps.status@1100.1.31
  - @pnpm/engine.runtime.commands@1101.1.10
  - @pnpm/engine.runtime.system-version@1100.0.14
  - @pnpm/exec.esm-node-path-loader@1100.0.2
  - @pnpm/exec.lifecycle@1100.1.25
  - @pnpm/exec.npm-lifecycle@1100.0.6
  - @pnpm/exec.pnpm-cli-runner@1100.0.6
  - @pnpm/installing.client@1100.3.17
  - @pnpm/installing.commands@1101.4.7
  - @pnpm/pkg-manifest.reader@1100.0.23
  - @pnpm/store.path@1100.1.3
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.injected-deps-syncer@1100.0.45
  - @pnpm/workspace.project-manifest-reader@1100.1.4
  - @pnpm/workspace.projects-filter@1100.0.50
  - @pnpm/workspace.projects-reader@1101.1.5
  - @pnpm/workspace.projects-sorter@1101.0.2
  - @pnpm/workspace.task-scheduler@1100.0.5
