---
"pacquet": patch
---

The `nodeVersion` setting no longer changes the global virtual store directory names or the side-effects cache keys. These follow the project's Node.js runtime pin, then the `node` on `PATH`. `pnpm licenses` and `pnpm store status` now find packages that an install with `nodeVersion` placed in the global virtual store.
