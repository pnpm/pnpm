---
"@pnpm/store.controller": patch
"pnpm": patch
---

`pnpm store prune` no longer forgets a project that was installed with a different symlinked spelling of the store path, so it keeps the packages that project uses in the global virtual store.
