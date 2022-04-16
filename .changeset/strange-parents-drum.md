---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Don't update a direct dependency that has the same name as a dependency in the workspace, when adding a new dependency to a workspace project [#4575](https://github.com/pnpm/pnpm/pull/4575).

