---
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` no longer gives a project an optional peer dependency that it skipped because a required peer would conflict with the project's dependencies. Before, `dedupePeerDependents` merged the project's copy of the package that declares the peer with another project's copy that resolves it [#17781](https://github.com/pnpm/pnpm/issues/17781).
