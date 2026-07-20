---
"pacquet": minor
---

Added support for the `cleanupUnusedCatalogs` setting: when enabled, `pnpm add`, `pnpm update`, and `pnpm remove` drop catalog entries from `pnpm-workspace.yaml` that no workspace project references.
