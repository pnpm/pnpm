---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm prune --prod` and production installs now prune excluded development dependencies even when lockfile generation is disabled.
