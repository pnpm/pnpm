---
"@pnpm/deps.inspection.tree-builder": patch
"pnpm": patch
"pacquet": patch
---

`pnpm why` no longer merges distinct local `file:` directory dependencies that share the same package name across projects with dedicated lockfiles (`sharedWorkspaceLockfile: false`).
