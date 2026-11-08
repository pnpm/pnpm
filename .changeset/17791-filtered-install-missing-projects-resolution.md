---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --filter` in a sparse checkout now counts the dependencies of workspace projects that are missing from disk when it resolves peer dependencies and marks packages as optional. The lockfile it writes no longer differs from a full checkout in those entries [pnpm/pnpm#17791](https://github.com/pnpm/pnpm/issues/17791).
