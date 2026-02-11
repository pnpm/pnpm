---
"@pnpm/reviewing.dependencies-hierarchy": patch
"@pnpm/list": patch
"pnpm": patch
---

Optimize `pnpm why` and `pnpm list` performance in workspaces with many importers by sharing the dependency graph and materialization cache across all importers instead of rebuilding them independently for each one.
