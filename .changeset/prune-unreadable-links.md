---
"@pnpm/store.controller": patch
"pnpm": patch
"pacquet": patch
---

`pnpm store prune` now stops if a dependency link cannot be read. This keeps installed packages from being removed after an incomplete scan.
