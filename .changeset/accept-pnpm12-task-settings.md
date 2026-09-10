---
"@pnpm/config.reader": patch
"@pnpm/types": patch
"pnpm": patch
---

pnpm no longer fails on a `pnpm-workspace.yaml` whose `tasks` entries declare `outputs`, `inputs`, `env`, `cache`, or `cargoTargetDir`. These pnpm 12 settings are now accepted and ignored.
