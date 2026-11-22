---
"@pnpm/deps.graph-builder": patch
"@pnpm/deps.graph-hasher": patch
"pnpm": patch
---

Fixed `link:` dependencies under `enableGlobalVirtualStore` so linked children are materialized and slots remain isolated by their resolved link targets.
