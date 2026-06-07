---
"@pnpm/config.reader": patch
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

Avoid writing `packageManagerDependencies` to `pnpm-lock.yaml` when package manager policy is set to `onFail: ignore` or `pmOnFail: ignore` [#12228](https://github.com/pnpm/pnpm/issues/12228).
