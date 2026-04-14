---
"@pnpm/store.commands": patch
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

Fixed `pnpm store prune` removing packages used by the globally installed pnpm, breaking it.
