---
"pacquet": patch
"@pnpm/napi": patch
---

`pnpm install` with `lockfile.format: v13` now writes `pnpm-lock.yaml` in the v13 format when it updates an existing v9 lockfile without resolving, for example after a dependency is removed. Such an install used to keep the v9 format.
