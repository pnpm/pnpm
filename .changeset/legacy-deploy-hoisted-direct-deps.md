---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

A legacy `pnpm deploy` with `node-linker=hoisted` now puts the deployed project's direct dependencies at the top of the deployed `node_modules` [#9671](https://github.com/pnpm/pnpm/issues/9671).
