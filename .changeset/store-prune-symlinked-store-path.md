---
"pacquet": patch
---

`pnpm store prune` now keeps the packages of a project when the store path reaches its directory through a symlink. Previously, prune dropped such a project from its list and removed the packages only that project used from the global virtual store.
