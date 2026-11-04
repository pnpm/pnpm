---
"pacquet": patch
---

With `verifyDepsBeforeRun` set, `pnpm run` and `pnpm exec` in an up-to-date workspace no longer read the `package.json` of each project whose file did not change since the last install [pnpm/tasks#256](https://github.com/pnpm/tasks/issues/256).
