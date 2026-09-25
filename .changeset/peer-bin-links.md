---
"@pnpm/installing.deps-restorer": patch
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now links the executables of auto-installed peer dependencies into the workspace root's `node_modules/.bin`, including after a frozen-lockfile reinstall [#8511](https://github.com/pnpm/pnpm/issues/8511).
