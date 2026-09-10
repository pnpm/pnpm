---
"@pnpm/config.reader": patch
"@pnpm/types": patch
"pnpm": patch
---

pnpm no longer fails on a `pnpm-workspace.yaml` whose `tasks` entries declare `outputs`, `inputs`, `env`, `cache`, or `cargoTargetDir`. These settings configure the pnpm 12 task cache, and pnpm 11 now accepts and ignores them so one workspace can be shared by both versions.
