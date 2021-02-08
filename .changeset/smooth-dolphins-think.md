---
"@pnpm/modules-cleaner": minor
---

`prune()` accepts a new option: `pruneVirtualStore`. When `pruneVirtualStore` is `true`, any unreferenced packages are removed from the virtual store (from `node_modules/.pnpm`).
