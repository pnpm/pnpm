---
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

With `dedupePeerDependents: false`, a missing optional peer is no longer resolved from a version that only another workspace project depends on. The workspace root's dependencies still count if `resolvePeersFromWorkspaceRoot` is `true`. Before, a tool in one project's `devDependencies`, such as `webpack-cli`, could become an optional peer of another project's production dependencies [pnpm/pnpm#16666](https://github.com/orgs/pnpm/discussions/16666).
