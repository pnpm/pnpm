---
"@pnpm/deps.inspection.tree-builder": patch
"@pnpm/deps.inspection.list": patch
"@pnpm/deps.inspection.commands": patch
"pnpm": patch
---

Fixed `pnpm why --exclude-peers` to exclude peer dependency edges from the reverse dependency tree.
