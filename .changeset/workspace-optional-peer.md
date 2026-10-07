---
"pacquet": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm install` and `pnpm dedupe` now preserve workspace packages that provide optional peers through the workspace root [pnpm/pnpm#16706](https://github.com/pnpm/pnpm/issues/16706).
