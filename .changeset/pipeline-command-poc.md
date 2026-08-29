---
"pacquet": minor
---

Added an experimental `pnpm pipeline [name]` command that runs a named set of workspace tasks the way a CI run would: a frozen install first, then only the projects affected since the merge base with `pipelineBase` (falling back to every project when workspace-root files changed), scheduled over the task graph without bailing, with cached task results restored — declared `outputs` files and replayed logs — instead of re-run. Pipelines are declared under the new `pipelines` setting in `pnpm-workspace.yaml`, and `tasks` entries gain `outputs`, `inputs`, `env`, and `cache` fields that control the task cache.
