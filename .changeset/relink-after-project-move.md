---
"@pnpm/deps.status": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install` and `pnpm run` now reinstall a single project that was moved or renamed together with its `node_modules`. Before, they reported "Already up to date" while links such as Windows junctions still pointed at the old location [#9512](https://github.com/pnpm/pnpm/issues/9512).
