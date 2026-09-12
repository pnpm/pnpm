---
"pacquet": patch
---

`pnpm dedupe` now processes all workspace projects by default, including workspaces with a separate lockfile per project. Workspace filters now select which projects to process. `--fail-if-no-match` now exits with an error when no projects match [pnpm/pnpm#14732](https://github.com/pnpm/pnpm/issues/14732).
