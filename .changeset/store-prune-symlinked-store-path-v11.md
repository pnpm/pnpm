---
"@pnpm/store.controller": patch
"pnpm": patch
---

`pnpm store prune` now keeps the packages of a project that was installed with a different symlinked spelling of the store path. Previously, prune dropped such a project from its list and removed the packages only that project used from the global virtual store.
