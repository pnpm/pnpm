---
"pacquet": patch
---

`pnpm install` now skips the Cargo and Python projects inside a nested directory that has its own `pnpm-workspace.yaml` or `.git` directory, such as a git worktree of the same workspace or a separate clone.
