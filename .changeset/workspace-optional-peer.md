---
"pacquet": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm install` and `pnpm dedupe` now link an optional peer to the workspace package that the workspace root depends on when the picked version matches it. Previously they installed the registry package with the same name and version [pnpm/pnpm#16706](https://github.com/pnpm/pnpm/issues/16706).
