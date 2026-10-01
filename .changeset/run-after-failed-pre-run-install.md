---
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm run` and `pnpm exec` now warn and run the command when the install that `verifyDepsBeforeRun` starts fails. This lets scripts run in sandboxes where pnpm cannot install, such as containers with a read-only store or no network [#15173](https://github.com/pnpm/pnpm/issues/15173).
