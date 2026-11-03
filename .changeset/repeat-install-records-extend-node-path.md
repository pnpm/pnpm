---
"@pnpm/workspace.state": patch
"pacquet": patch
"pnpm": patch
---

A repeat `pnpm install` now rewrites the bin shims after `extendNodePath` changes. Before, it reported "Already up to date" and the shims kept the `NODE_PATH` of the previous setting.
