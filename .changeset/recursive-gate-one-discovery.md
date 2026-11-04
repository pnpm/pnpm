---
"pacquet": patch
---

A recursive `pnpm run` or `pnpm exec` with `verifyDepsBeforeRun` set no longer reads every `package.json` in the workspace once per project when `sharedWorkspaceLockfile` is `false`. In a 300-project workspace, `pnpm -r exec true` went from 490 ms to 42 ms [pnpm/tasks#256](https://github.com/pnpm/tasks/issues/256).
