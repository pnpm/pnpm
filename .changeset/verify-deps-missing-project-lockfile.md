---
"@pnpm/deps.status": patch
"pnpm": patch
---

`verifyDepsBeforeRun` now reports the dependencies as outdated when `sharedWorkspaceLockfile` is `false` and the `pnpm-lock.yaml` of a project the command runs is missing. Previously the check reported them as up to date, so `pnpm run` skipped the install [pnpm/tasks#138](https://github.com/pnpm/tasks/issues/138).
