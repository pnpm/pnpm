---
"@pnpm/deps.inspection.commands": patch
"@pnpm/deps.inspection.list": minor
"@pnpm/deps.inspection.tree-builder": minor
"pnpm": patch
"pacquet": patch
---

`pnpm why` now reads each selected workspace project's lockfile and modules directory when `sharedWorkspaceLockfile` is `false` [pnpm/tasks#169](https://github.com/pnpm/tasks/issues/169).
