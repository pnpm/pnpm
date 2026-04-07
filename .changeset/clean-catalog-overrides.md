---
"@pnpm/workspace.workspace-manifest-writer": patch
"pnpm": patch
---

Prevent catalog entries from being removed by `cleanupUnusedCatalogs` when they are referenced only from workspace `overrides` in `pnpm-workspace.yaml`.
