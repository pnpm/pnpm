---
"@pnpm/store.commands": patch
"pnpm": patch
---

`pnpm store prune` now removes the packages that only expired `pnpm dlx` cache entries used, as long as the store still has another registered project. They were left in the store until the next `pnpm store prune` [#16383](https://github.com/pnpm/pnpm/discussions/16383).
