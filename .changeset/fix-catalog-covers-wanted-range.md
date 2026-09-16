---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm add <pkg>` now reuses an existing catalog entry when the catalog range covers the wanted version [pnpm/pnpm#14865](https://github.com/pnpm/pnpm/issues/14865).
