---
"pacquet": patch
"@pnpm/napi": patch
---

`pnpm install` returns "Already up to date" again in a workspace with injected workspace dependencies and a shared lockfile. Since v12.7.0 every repeat install in such a workspace ran the full install and copied the injected projects again.
