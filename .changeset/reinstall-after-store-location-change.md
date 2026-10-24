---
"@pnpm/workspace.state": patch
"@pnpm/deps.status": patch
"@pnpm/exec.commands": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install` now recreates `node_modules` after a `storeDir` or `virtualStoreDir` change. It used to report "Already up to date".
