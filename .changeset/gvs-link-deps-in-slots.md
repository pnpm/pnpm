---
"@pnpm/deps.graph-builder": patch
"@pnpm/deps.graph-hasher": patch
"pnpm": patch
"pacquet": patch
---

Fixed `link:` dependencies under `enableGlobalVirtualStore` so linked children are materialized and slots remain isolated by their resolved link targets.
