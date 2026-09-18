---
"pacquet": patch
"@pnpm/napi": patch
---

`pnpm install` no longer refuses the repeat-install fast path just because a changed `pnpm-lock.yaml` is 16 MiB or larger. Such a lockfile forced a full install on the run after every change.
