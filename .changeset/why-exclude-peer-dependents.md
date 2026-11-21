---
"@pnpm/deps.inspection.tree-builder": patch
"@pnpm/deps.inspection.list": patch
"@pnpm/deps.inspection.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm why --exclude-peers` now leaves out packages that are reached only as a resolved peer dependency. Previously the flag was accepted but ignored.
