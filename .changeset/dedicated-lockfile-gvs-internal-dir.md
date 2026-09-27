---
"pacquet": patch
---

With `enableGlobalVirtualStore` and `sharedWorkspaceLockfile: false`, each project now keeps its current lockfile and its hidden hoisted dependencies in its own `node_modules/.pnpm`. Before this fix, every project wrote them to the workspace root's `node_modules/.pnpm`, so each install treated the other projects' packages as its own, and concurrent project installs could fail with `EEXIST` [#14480](https://github.com/pnpm/pnpm/issues/14480).
