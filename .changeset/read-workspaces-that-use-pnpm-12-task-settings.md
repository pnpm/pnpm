---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now reads a `pnpm-workspace.yaml` whose `tasks` section uses a setting only pnpm 12 acts on, such as `concurrencyGroup`. A task's unrecognized fields are ignored, unless the field only differs in case from `concurrency` or `dependsOn`, which pnpm reports as a typo.

The warning about unrecognized top-level settings now names `cargo`, `concurrencyGroups`, and `pipelines` as pnpm 12 settings.
