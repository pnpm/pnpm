---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now reads a `pnpm-workspace.yaml` whose `tasks` section uses a setting only pnpm 12 acts on, such as `concurrencyGroup`. A task's unrecognized fields are ignored. `concurrency` and `dependsOn` are still checked.

The warning about unrecognized top-level settings now names `cargo`, `concurrencyGroups`, and `pipelines` as pnpm 12 settings. It used to offer a similar pnpm 11 setting as a spelling correction.
