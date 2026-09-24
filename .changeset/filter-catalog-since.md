---
"@pnpm/workspace.projects-filter": patch
"pnpm": patch
"pacquet": patch
---

`pnpm --filter "[<since>]"` now selects workspace packages when dependency versions change in a catalog in `pnpm-workspace.yaml` [#8718](https://github.com/pnpm/pnpm/issues/8718).
