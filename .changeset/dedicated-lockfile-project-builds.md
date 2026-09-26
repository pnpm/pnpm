---
"pacquet": patch
---

`pnpm rebuild`, `pnpm approve-builds`, and `pnpm ignored-builds` now work on the current project's `node_modules` when they run inside a project of a workspace with `sharedWorkspaceLockfile: false`. They used to read the workspace root's `node_modules`, so `pnpm rebuild` did not rebuild the project's dependencies and created a second virtual store at the workspace root [#9402](https://github.com/pnpm/pnpm/issues/9402).
