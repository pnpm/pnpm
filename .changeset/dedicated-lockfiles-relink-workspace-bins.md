---
"pacquet": patch
---

`pnpm install` now relinks a command that a workspace project renames into the projects that depend on it when `sharedWorkspaceLockfile` is `false` [pnpm/tasks#224](https://github.com/pnpm/tasks/issues/224).
