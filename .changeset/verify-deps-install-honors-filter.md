---
"@pnpm/deps.status": patch
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm run` and `pnpm exec` now install only the projects selected by `--filter` when `verifyDepsBeforeRun` triggers an install, and they no longer treat a project a previous filtered install never materialized as up to date [#11865](https://github.com/pnpm/pnpm/issues/11865).
