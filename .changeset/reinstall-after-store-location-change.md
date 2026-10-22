---
"@pnpm/workspace.state": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install` now recreates `node_modules` after a `storeDir` or `virtualStoreDir` change. It used to report "Already up to date", while `pnpm add` failed with `ERR_PNPM_UNEXPECTED_STORE` and told you to run `pnpm install`.
