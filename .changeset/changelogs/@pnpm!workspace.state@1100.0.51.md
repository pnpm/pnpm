## 1100.0.51

### Patch Changes

- `pnpm run`, `pnpm exec`, and `pnpm restart` now pass the install flags they were given, such as `--lockfile-dir`, `--modules-dir`, `--store-dir`, `--offline`, and `--ignore-scripts`, to the `pnpm install` that `verifyDepsBeforeRun: install` starts. The check that runs before a script now holds the workspace projects the script's project depends on. It agrees with the repeat-install check on which projects a filtered install left uninstalled, including with per-project lockfiles. Workspace dependencies declared with `link:` are now included in the workspace project graph, so `--filter=<project>...` selects them and, with `sharedWorkspaceLockfile: false` and `injectWorkspacePackages: true`, the check before a script still requires them to be installed. A filtered install no longer forgets the ignored build scripts of the projects it did not select.

- The `verifyDepsBeforeRun` check now notices an edited manifest, lockfile, or patch file even when the edit kept the file's modification time, as `cp -p`, `rsync -a`, or an archive extraction leave it.

  After `pnpm install --filter`, `pnpm run` and `pnpm exec` now notice a changed manifest in a project that install left out. A filtered command for a project that install did not reinstall after a lockfile change now installs first.

  `pnpm dedupe --check` and `pnpm install --dry-run` no longer record the dependencies as installed, so the `verifyDepsBeforeRun` check that follows them still notices outdated dependencies.

- Updated dependencies:
  - @pnpm/config.reader@1102.3.6
  - @pnpm/types@1102.1.2
