---
"pacquet": patch
---

`pnpm install` with `optimisticRepeatInstall` now re-runs when `configDependencies` changed since the previous install, matching `verifyDepsBeforeRun`.

`verifyDepsBeforeRun` now reports `node_modules` out of date when `useGitBranchLockfile` is enabled and the branch lockfile is missing.
