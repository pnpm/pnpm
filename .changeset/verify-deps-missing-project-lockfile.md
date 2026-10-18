---
"@pnpm/deps.status": patch
"pnpm": patch
---

`verifyDepsBeforeRun` now reports a workspace as outdated when `sharedWorkspaceLockfile` is `false` and a project's `pnpm-lock.yaml` is missing. Previously the check reported the dependencies as up to date, so `pnpm run` skipped the install [pnpm/tasks#138](https://github.com/pnpm/tasks/issues/138).
