---
"pacquet": patch
---

`pnpm add <pkg>@<tag>` now uses a catalog entry set to that same dist-tag. The dependency is saved as `catalog:`, and the entry records the version the tag resolved to [pnpm/tasks#225](https://github.com/pnpm/tasks/issues/225).
