---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm no longer fails to read a `pnpm-workspace.yaml` whose `tasks` section uses a setting only pnpm 12 acts on, such as `concurrencyGroup`. A task's unrecognized fields are ignored, and `concurrency` and `dependsOn` are still checked.

The warning about unrecognized top-level settings now names `cargo`, `concurrencyGroups`, and `pipelines` as pnpm 12 settings instead of suggesting a similar pnpm 11 setting.
