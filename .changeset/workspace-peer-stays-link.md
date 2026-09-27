---
"pacquet": patch
---

A fresh `pnpm install` keeps a workspace peer as `link:` when injected workspace packages are deduped. pnpm 12 wrote that dependency as a peer-suffixed `file:` copy, because the copy picked up an optional peer the workspace project itself did not [#10433](https://github.com/pnpm/pnpm/issues/10433).
