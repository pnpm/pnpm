---
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` no longer fills in a missing optional peer dependency if a required peer it brings in conflicts with a version the project depends on. Before, such a peer was installed along with it and reported as unmet [pnpm/pnpm#17718](https://github.com/orgs/pnpm/discussions/17718).
