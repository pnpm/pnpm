---
"pacquet": patch
---

`pnpm update <pkg>` no longer rewrites the lockfile entry of a package it did not update, when that package also lists one of its dependencies as an optional peer. `pnpm dedupe --check` reported such an entry as changed after every update [#16910](https://github.com/pnpm/pnpm/issues/16910).
