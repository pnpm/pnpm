---
"@pnpm/store.commands": patch
"@pnpm/store.controller": patch
"pnpm": patch
"pacquet": patch
---

`pnpm store prune` now removes the packages that only expired `pnpm dlx` cache entries used. They were left in the store until the next `pnpm store prune` [#16383](https://github.com/pnpm/pnpm/discussions/16383).

`pnpm store prune` also removes global virtual store packages once every project registered in the store has been deleted. It used to keep them all.
