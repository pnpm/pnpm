---
"@pnpm/package-store": patch
"pnpm": patch
---

`pnpm store prune` should not fail if there are unexpected subdirectories in the content-addressable store.
