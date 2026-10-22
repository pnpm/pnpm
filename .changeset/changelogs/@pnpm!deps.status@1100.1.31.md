## 1100.1.31

### Patch Changes

- `verifyDepsBeforeRun` and repeat `pnpm install` runs now apply `overrides` before comparing a modified `package.json` with `pnpm-lock.yaml` [#17127](https://github.com/pnpm/pnpm/issues/17127).

- `verifyDepsBeforeRun` no longer treats the workspace root as up to date after a filtered install with `pruneDirectDependencies: true` skipped it [pnpm/tasks#181](https://github.com/pnpm/tasks/issues/181).

- `pnpm run`, `pnpm exec`, and `pnpm restart` now pass the install flags they were given, such as `--lockfile-dir`, `--modules-dir`, `--store-dir`, `--offline`, and `--ignore-scripts`, to the `pnpm install` that `verifyDepsBeforeRun: install` starts. The check that runs before a script now holds the workspace projects the script's project depends on. It agrees with the repeat-install check on which projects a filtered install left uninstalled, including with per-project lockfiles. Workspace dependencies declared with `link:` are now included in the workspace project graph, so `--filter=<project>...` selects them and, with `sharedWorkspaceLockfile: false` and `injectWorkspacePackages: true`, the check before a script still requires them to be installed. A filtered install no longer forgets the ignored build scripts of the projects it did not select.

- `verifyDepsBeforeRun` now compares a patch file that changed on disk with the patch hash in the lockfile. A patch that was copied or touched without changing its content no longer marks dependencies as outdated [#16988](https://github.com/pnpm/pnpm/issues/16988).

- `verifyDepsBeforeRun` now checks whether each importer's resolved catalog versions match the catalog snapshot in `pnpm-lock.yaml`.

  `verifyDepsBeforeRun` now respects `lockfileDir` in single-project repositories.

  When the install that runs before a script leaves `node_modules` out of date, pnpm now prints a warning.

- The `verifyDepsBeforeRun` check now notices an edited manifest, lockfile, or patch file even when the edit kept the file's modification time, as `cp -p`, `rsync -a`, or an archive extraction leave it.

  After `pnpm install --filter`, `pnpm run` and `pnpm exec` now notice a changed manifest in a project that install left out. A filtered command for a project that install did not reinstall after a lockfile change now installs first.

  `pnpm dedupe --check` and `pnpm install --dry-run` no longer record the dependencies as installed, so the `verifyDepsBeforeRun` check that follows them still notices outdated dependencies.

- `verifyDepsBeforeRun` now reports the dependencies as outdated when `sharedWorkspaceLockfile` is `false` and the `pnpm-lock.yaml` of a project the command runs is missing. Previously the check reported them as up to date, so `pnpm run` skipped the install [pnpm/tasks#138](https://github.com/pnpm/tasks/issues/138).

- Updated dependencies:
  - @pnpm/config.reader@1102.3.6
  - @pnpm/deps.path@1101.0.7
  - @pnpm/hooks.read-package-hook@1100.3.9
  - @pnpm/installing.context@1101.0.12
  - @pnpm/lockfile.fs@1100.2.14
  - @pnpm/lockfile.settings-checker@1100.2.14
  - @pnpm/lockfile.verification@1100.1.13
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.projects-reader@1101.1.5
  - @pnpm/workspace.state@1100.0.51
  - @pnpm/workspace.workspace-manifest-reader@1100.3.0
