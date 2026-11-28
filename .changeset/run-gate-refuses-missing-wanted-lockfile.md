---
"pnpm": patch
---

`pnpm run` and `pnpm exec` now treat a missing lockfile as dependencies that are not up to date when `verifyDepsBeforeRun` is enabled. The `install` action restores the lockfile before running the command, and the `error` action refuses to run [pnpm/tasks#219](https://github.com/pnpm/tasks/issues/219).
