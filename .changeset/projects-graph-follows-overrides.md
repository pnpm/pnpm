---
"@pnpm/hooks.read-package-hook": patch
"@pnpm/workspace.projects-graph": patch
"@pnpm/workspace.projects-filter": patch
"@pnpm/installing.commands": patch
"pacquet": patch
"pnpm": patch
---

The workspace project graph now follows `overrides`. An override that points a dependency at a workspace package, such as `foo: workspace:*`, makes that package a dependency of every project that declares `foo`, whatever range those projects declare and whether `linkWorkspacePackages` is on. `pnpm install` now runs that package's lifecycle scripts before the scripts of the projects depending on it. `pnpm -r run` orders the projects the same way, and `--filter` selectors that follow dependencies see the edge.
