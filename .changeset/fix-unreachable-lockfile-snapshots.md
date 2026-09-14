---
"pacquet": patch
---

`verifyDepsBeforeRun` no longer triggers an unnecessary reinstall loop when `pnpm-lock.yaml` contains unreachable lockfile snapshots [#14891](https://github.com/pnpm/pnpm/issues/14891).
