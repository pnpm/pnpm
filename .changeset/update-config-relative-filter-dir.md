---
"@pnpm/workspace.projects-filter": patch
"pnpm": patch
"pacquet": patch
---

Relative directory selectors set on `config.filter` or `config.filterProd` by an `updateConfig` hook now resolve from the workspace root even when `--dir` is passed or `pnpm` runs from a subdirectory [#17906](https://github.com/pnpm/pnpm/issues/17906).
