---
"@pnpm/workspace.state": patch
"pnpm": patch
"pacquet": patch
---

A repeat `pnpm install` now runs a full install when the `runtimeOnFail` setting changes. Before, it printed "Already up to date" and did not install the runtime a project declares in `devEngines.runtime` [pnpm/tasks#303](https://github.com/pnpm/tasks/issues/303).
