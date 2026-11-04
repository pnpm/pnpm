---
"pacquet": patch
---

A recursive `pnpm run` or `pnpm exec` with `verifyDepsBeforeRun` set now reads the workspace's `package.json` files once in a workspace with `sharedWorkspaceLockfile: false`. It read them again for every project, which made `pnpm -r exec` in a 300-project workspace take 490 ms instead of 42 ms [pnpm/tasks#256](https://github.com/pnpm/tasks/issues/256).
