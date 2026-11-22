---
"pnpm": patch
"@pnpm/store.index": patch
"@pnpm/deps.inspection.commands": patch
"@pnpm/deps.inspection.list": patch
"@pnpm/deps.inspection.tree-builder": patch
---

`pnpm list --lockfile-only --find-by` now reads manifests from the configured store when `node_modules` is absent.
