---
"@pnpm/bins.linker": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

With `nodeLinker: hoisted`, `hoistWorkspacePackages` now links each workspace project that `hoistPattern` or `publicHoistPattern` selects into the root `node_modules`, unless a hoisted package or a root dependency already uses its name. The project's bins are linked into the root `node_modules/.bin` [#7553](https://github.com/pnpm/pnpm/issues/7553).
