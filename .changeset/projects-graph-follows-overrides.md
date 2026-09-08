---
"@pnpm/hooks.read-package-hook": patch
"@pnpm/workspace.projects-graph": patch
"@pnpm/workspace.projects-filter": patch
"@pnpm/installing.commands": patch
"pacquet": patch
"pnpm": patch
---

The workspace project graph now follows `overrides`. An override that points a dependency at a workspace package, such as `foo: workspace:*` or `foo: link:./packages/foo`, makes that package a dependency of every project declaring `foo`, whatever range those projects declare and whether or not `linkWorkspacePackages` is on. `pnpm install` runs the lifecycle scripts of such a dependency before the scripts of the projects depending on it, `pnpm -r run` orders them the same way, and `--filter` selectors that follow dependencies see the edge. Before, only the declared range counted, so the scripts could run at the same time and a `prepare` reading a file another project's `prepare` generates failed.
