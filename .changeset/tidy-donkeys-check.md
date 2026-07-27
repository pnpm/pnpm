---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Prevented `pnpm dedupe --check` from removing an incompatible `node_modules` directory.
