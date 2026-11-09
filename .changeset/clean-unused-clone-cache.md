---
"pacquet": patch
---

Local installs on macOS now keep cached clones separate from the global virtual store. `pnpm store prune` removes this cache, preventing unused cached packages from accumulating [pnpm/pnpm#17907](https://github.com/pnpm/pnpm/issues/17907).
