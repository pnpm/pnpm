---
"pacquet": patch
---

`pnpm` now warns when a project's `pnpm-workspace.yaml` sets `ci`, `globalShims`, or `tools`, and names where to set them instead. These settings were already ignored there without a warning. `pnpm config set` also refuses to write them to a project's `pnpm-workspace.yaml`.
