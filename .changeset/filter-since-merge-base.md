---
"@pnpm/workspace.projects-filter": patch
"pacquet": patch
"pnpm": patch
---

The `[<since>]` filter selector now compares against the commit where the current branch forked from `<since>`. Projects changed only by newer commits on `<since>` are no longer selected. Uncommitted changes are still included. In a shallow clone without that commit, pnpm compares against `<since>` directly, as before [#9907](https://github.com/pnpm/pnpm/issues/9907).
