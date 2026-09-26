---
"@pnpm/deps.inspection.commands": patch
"@pnpm/deps.inspection.list": patch
"@pnpm/deps.inspection.tree-builder": patch
"pnpm": patch
"pacquet": patch
---

`pnpm list --only-projects` now lists a workspace project that sets `publishConfig.directory`. Dependents link such a project through its publish directory, which `--only-projects` did not recognize as a project [#10635](https://github.com/pnpm/pnpm/issues/10635).
