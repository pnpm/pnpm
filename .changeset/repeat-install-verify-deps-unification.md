---
"@pnpm/deps.status": patch
"@pnpm/exec.commands": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/workspace.projects-graph": patch
"@pnpm/workspace.state": patch
"pnpm": patch
"pacquet": patch
---

`pnpm run`, `pnpm exec`, and `pnpm restart` now pass the install flags they were given, such as `--lockfile-dir`, `--modules-dir`, `--store-dir`, `--offline`, and `--ignore-scripts`, to the `pnpm install` that `verifyDepsBeforeRun: install` starts. The check that runs before a script now holds the workspace projects the script's project depends on. It agrees with the repeat-install check on which projects a filtered install left uninstalled, including with per-project lockfiles. Workspace dependencies declared with `link:` are now included in the workspace project graph, so `--filter=<project>...` selects them and, with `sharedWorkspaceLockfile: false` and `injectWorkspacePackages: true`, the check before a script still requires them to be installed. A filtered install no longer forgets the ignored build scripts of the projects it did not select.
