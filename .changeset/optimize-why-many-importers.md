---
"@pnpm/deps.inspection.dependencies-hierarchy": patch
"@pnpm/deps.inspection.list": patch
"pnpm": patch
---

Optimize `pnpm why` and `pnpm list` performance in workspaces with many importers by sharing the dependency graph and materialization cache across all importers instead of rebuilding them independently for each one [#10596](https://github.com/pnpm/pnpm/pull/10596/changes).
