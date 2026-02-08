---
"@pnpm/resolve-dependencies": patch
"@pnpm/core": patch
"pnpm": patch
---

Fix workspace package protocol consistency when using `injectWorkspacePackages`

Previously, workspace packages would inconsistently switch between `link:` and `file:` protocols after operations like `pnpm rm` when `injectWorkspacePackages` was enabled. The issue was that deduplication logic couldn't identify workspace packages in single-package operation contexts.

This fix ensures workspace packages maintain consistent protocols by checking against all workspace packages from the lockfile, not just packages in the current operation context.

Fixes #9518
