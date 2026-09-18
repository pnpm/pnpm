---
"pacquet": patch
"@pnpm/napi": patch
---

`pnpm install` now takes the repeat-install fast path after a `pnpm-lock.yaml` of 16 MiB or more changes. Such a lockfile forced a full install on the run after every change.
