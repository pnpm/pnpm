---
"pacquet": patch
---

`pnpm install` now locks Cargo path dependencies outside the workspace, as Cargo does. It failed with `workspace dependency <name> is not a workspace member`. `pnpm install --frozen-lockfile` now also fails when `Cargo.lock` misses a path dependency or locks a git dependency at another branch, tag, or revision.
