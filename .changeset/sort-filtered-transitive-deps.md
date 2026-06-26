---
"@pnpm/workspace.projects-sorter": patch
"@pnpm/workspace.projects-filter": patch
"@pnpm/config.reader": patch
"@pnpm/exec.commands": patch
"@pnpm/building.commands": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fixed the topological order of `--filter`ed commands (`pnpm run`, `pnpm exec`, `pnpm publish`, `pnpm pack`, `pnpm rebuild`) when the selected projects depend on each other only transitively through projects that were not selected. Previously such selected projects could run concurrently or in the wrong order; now a project always runs after the selected projects it transitively depends on, while projects without a real dependency relationship still run concurrently. This now also holds for prod-only filters (`--filter-prod`), which resolve order through the production dependency graph so transitive production dependencies are respected without pulling back the dev dependencies the filter drops, and for selections that mix `--filter` with `--filter-prod` [#8335](https://github.com/pnpm/pnpm/issues/8335).
