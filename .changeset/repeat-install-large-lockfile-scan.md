---
"pacquet": patch
"@pnpm/napi": patch
---

The repeat-install check now scans a changed `pnpm-lock.yaml` for merge conflict markers whatever its size. A lockfile of 16 MiB or more was treated as unverifiable and forced a full install on every run after it changed.
