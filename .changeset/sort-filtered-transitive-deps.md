---
"@pnpm/workspace.projects-sorter": patch
"@pnpm/exec.commands": patch
"@pnpm/building.commands": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fixed the topological order of `--filter`ed commands (`pnpm run`, `pnpm exec`, `pnpm publish`, `pnpm pack`, `pnpm rebuild`) when the selected projects depend on each other only transitively through projects that were not selected. Previously, such selected projects could run concurrently or in the wrong order; now a project always runs after the selected projects it transitively depends on. Projects without a real dependency relationship still run concurrently [#8335](https://github.com/pnpm/pnpm/issues/8335).
