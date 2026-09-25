---
"pacquet": patch
---

`node_modules/.package-map.json` no longer contains entries that point at directories that do not exist. Such entries appeared for packages installed only with peer dependencies, most visibly with `enableGlobalVirtualStore` [#14938](https://github.com/pnpm/pnpm/issues/14938).
