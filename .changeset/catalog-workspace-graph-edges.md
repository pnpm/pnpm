---
"@pnpm/workspace.projects-graph": patch
"@pnpm/workspace.projects-filter": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

A dependency declared with `catalog:` now counts as a workspace dependency when its catalog entry points at a workspace project, for example `workspace:*`. `pnpm -r run` runs that project first. `--filter <pkg>...` selects it [#15587](https://github.com/pnpm/pnpm/issues/15587).
