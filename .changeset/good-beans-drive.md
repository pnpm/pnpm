---
"@pnpm/store.cafs": patch
"@pnpm/store.commands": patch
"@pnpm/worker": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm store status` reporting approved builds of git dependencies as modified when the store contains legacy build metadata.
