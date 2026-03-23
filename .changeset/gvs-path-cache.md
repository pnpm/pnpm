---
"@pnpm/deps.graph-builder": patch
"pnpm": patch
---

Cache GVS directory paths in the pnpm store to skip hash recomputation on warm reinstalls.
