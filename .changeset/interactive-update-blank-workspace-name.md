---
"@pnpm/deps.inspection.outdated": patch
"pacquet": patch
"pnpm": patch
---

The `Workspace` column of `pnpm update --interactive` now falls back to the project's path when its `name` is only whitespace, as it already did for a missing or empty one — all three render an equally blank label otherwise.
