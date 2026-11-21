---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm install` now records the supply-chain verification of the lockfile it writes by the lockfile's content. Before, a lockfile that another process put in its place right after the write could reuse that verification on the next install.
