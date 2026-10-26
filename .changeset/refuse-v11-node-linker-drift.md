---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm add`, `pnpm remove`, and `pnpm update` now fail when the `node-linker` setting differs from the one recorded in `node_modules` [pnpm/tasks#203](https://github.com/pnpm/tasks/issues/203).
