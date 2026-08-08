---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm prune` is now recursive by default in workspaces. This fixes a bug where `pnpm prune --prod` executed in the root of a workspace would delete workspace-package symlinks in other packages that are production dependencies.
