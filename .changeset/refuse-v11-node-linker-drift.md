---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm add`, `pnpm remove`, and `pnpm update` now fail when the `node-linker` setting differs from the one recorded in `node_modules`. A `pnpm install` of the whole workspace recreates `node_modules` with the new `node-linker`, and an install filtered to some projects fails [pnpm/tasks#203](https://github.com/pnpm/tasks/issues/203).
