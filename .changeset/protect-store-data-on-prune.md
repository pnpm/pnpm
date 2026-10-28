---
"@pnpm/store.controller": patch
"pnpm": patch
---

`pnpm store prune` preserves other stores and global package data when `cacheDir` points to their parent directory.
