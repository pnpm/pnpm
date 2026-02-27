---
"@pnpm/plugin-commands-store": minor
"pnpm": minor
---

Added `pnpm store warmup` command that pre-populates the global virtual store from a lockfile without creating node_modules. Useful for warming caches in Docker layers, CI steps, or pre-build scripts.
