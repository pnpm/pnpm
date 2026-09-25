---
"@pnpm/workspace.projects-graph": patch
"pnpm": patch
"pacquet": patch
---

With `linkWorkspacePackages` enabled, a dependency declared as an `npm:` alias of a workspace project, such as `"math-alias": "npm:math@^1.0.0"`, now counts as a workspace dependency. `pnpm -r run` runs that project first. `--filter <pkg>...` selects it.
