---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

Under `nodeLinker: hoisted`, `pnpm install` will now correctly scan for and prune any physical orphaned nested package directories inside `packages/*/node_modules/` left behind by an interrupted or failed previous install.
