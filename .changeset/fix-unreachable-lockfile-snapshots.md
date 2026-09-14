---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now prunes a `pnpm-lock.yaml` snapshot that no importer reaches. It used to reimport that snapshot on every run and rerun the lifecycle scripts. `verifyDepsBeforeRun` also installed before every `pnpm run` and `pnpm exec` [#14891](https://github.com/pnpm/pnpm/issues/14891).
