---
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm run` and `pnpm exec` now install only the projects selected by `--filter` when `verifyDepsBeforeRun` triggers an install [#11865](https://github.com/pnpm/pnpm/issues/11865).
