---
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

Concurrent `pnpm run` and `pnpm exec` commands on out-of-date dependencies no longer start one install each. With `verifyDepsBeforeRun: install` (the default) or an accepted `prompt`, the first command installs while the others wait for it, then run once the dependencies are up to date. The installs used to race in the same `node_modules` and fail with filesystem errors [#14551](https://github.com/pnpm/pnpm/issues/14551).
