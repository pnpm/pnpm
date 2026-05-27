---
"@pnpm/deps.inspection.list": patch
"@pnpm/workspace.project-manifest-reader": patch
"pnpm": patch
---

Limit concurrent project manifest reads while listing large workspaces to avoid `EMFILE` errors.
