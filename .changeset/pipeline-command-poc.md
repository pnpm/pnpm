---
"pacquet": minor
---

Added `pnpm pipeline [name]` to install frozen dependencies and run workspace tasks declared in `pipelines`. It selects affected projects and runs their task graph without bailing on task failures.

Task settings now support `outputs`, `inputs`, `env`, and `cache`. Cache hits restore declared outputs and replay the task's logs.

`pnpm pipeline --dry-run` previews the task graph without installing configuration dependencies or executing workspace hooks.
