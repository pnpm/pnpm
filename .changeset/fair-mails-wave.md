---
"@pnpm/deps.graph-builder": patch
"pnpm": patch
---

`pnpm fetch` applies patches to packages that are only reachable through skipped local `file:` dependencies.
