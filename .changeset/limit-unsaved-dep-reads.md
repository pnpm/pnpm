---
"@pnpm/deps.inspection.tree-builder": patch
"pnpm": patch
---

`pnpm list` and `pnpm why` no longer crash with `EMFILE: too many open files` when a project has a large number of unsaved dependencies (packages present in `node_modules` but not in the lockfile). The reads of those packages are now concurrency-limited.
