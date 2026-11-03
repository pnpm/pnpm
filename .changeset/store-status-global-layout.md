---
"@pnpm/deps.graph-builder": patch
"@pnpm/store.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm store status` reporting untouched packages as modified when the global virtual store is enabled. [pnpm/pnpm#17631](https://github.com/pnpm/pnpm/issues/17631).
