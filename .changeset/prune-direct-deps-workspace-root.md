---
"@pnpm/deps.status": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

`verifyDepsBeforeRun` no longer treats the workspace root as up to date after a filtered install with `pruneDirectDependencies: true` skipped it [pnpm/tasks#181](https://github.com/pnpm/tasks/issues/181).
