---
"@pnpm/deps.status": patch
"@pnpm/exec.commands": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/workspace.state": patch
"pnpm": patch
"pacquet": patch
---

`verifyDepsBeforeRun: install` now forwards install-relevant CLI flags such as `--lockfile-dir`, `--modules-dir`, `--store-dir`, `--offline`, and `--ignore-scripts` when `pnpm run` or `pnpm exec` spawns `pnpm install`. Repeat installs and `verifyDepsBeforeRun` checks now agree across workspace dependency selections, dedicated lockfiles, and `CI=true` with `autoDedupe` [pnpm/tasks#161](https://github.com/pnpm/tasks/issues/161).
