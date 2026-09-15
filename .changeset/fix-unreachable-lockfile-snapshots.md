---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now removes a package from `node_modules` when no project in `pnpm-lock.yaml` depends on it any more. Such a package used to be reinstalled on every run, which reran the lifecycle scripts each time. `verifyDepsBeforeRun` also installed before every `pnpm run` and `pnpm exec` [#14891](https://github.com/pnpm/pnpm/issues/14891).
