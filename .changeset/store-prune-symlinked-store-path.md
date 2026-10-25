---
"pacquet": patch
---

`pnpm store prune` no longer forgets a project when the store path reaches its directory through a symlink, so it keeps the packages that project uses in the global virtual store.
