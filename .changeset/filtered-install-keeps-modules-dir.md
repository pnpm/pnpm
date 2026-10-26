---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

An install filtered to some workspace projects now fails if `node_modules` was installed with a different `nodeLinker`, `hoistPattern`, `publicHoistPattern`, store, or virtual store. Before, it recreated `node_modules` and left the projects it skipped without their dependencies. A `pnpm install` of the whole workspace still recreates `node_modules` [pnpm/tasks#217](https://github.com/pnpm/tasks/issues/217).
