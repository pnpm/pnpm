---
"pacquet": patch
"pnpm": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/deps.graph-sequencer": patch
---

Fixed peer deduplication giving a project different copies of a dependency across direct and transitive paths when an optional peer has no provider [pnpm/pnpm#17666](https://github.com/pnpm/pnpm/issues/17666).
