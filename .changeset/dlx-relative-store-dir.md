---
"@pnpm/exec.commands": patch
"pnpm": patch
---

`pnpm dlx` now resolves a relative `store-dir` against the current directory. Before, it created a separate store inside the dlx cache.
