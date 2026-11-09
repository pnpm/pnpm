---
"pacquet": patch
---

`pnpm store prune` now removes unused cached clones when registered projects do not use the global virtual store. This fixes cached packages accumulating on macOS with the default import method [pnpm/pnpm#17907](https://github.com/pnpm/pnpm/issues/17907).
