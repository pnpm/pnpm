---
"pnpm": patch
---

`pnpm install` and `verifyDepsBeforeRun` now decide freshness from the inputs each project's last install recorded. With `verifyDepsBeforeRun: error`, `pnpm run` and `pnpm exec` refuse to run when the project's lockfile is missing. The `install` action restores the lockfile before running the command [pnpm/tasks#219](https://github.com/pnpm/tasks/issues/219).
