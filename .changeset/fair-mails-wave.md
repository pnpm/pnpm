---
"@pnpm/deps.graph-builder": patch
"pnpm": patch
---

`pnpm fetch` now applies patches to packages that are only reachable through local `file:` dependencies [#11034](https://github.com/pnpm/pnpm/issues/11034).
