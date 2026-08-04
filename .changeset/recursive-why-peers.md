---
"pacquet": minor
---

Made `pnpm why` and `pnpm peers` recursive by default in workspaces. Recursive peer checks now honor workspace filters, and recursive `why` can inspect the active project when a workspace uses dedicated lockfiles.
