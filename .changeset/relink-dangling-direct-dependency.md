---
"@pnpm/deps.status": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now relinks a direct dependency whose link in `node_modules` points to a missing target. Before, it reported "Already up to date" and left the broken link [#9758](https://github.com/pnpm/pnpm/issues/9758).
