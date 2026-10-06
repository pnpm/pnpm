---
"pacquet": patch
---

`pnpm install` no longer installs the Cargo and Python dependencies of a nested directory that has its own `pnpm-workspace.yaml` or `.git` directory, such as a git worktree or a separate clone inside the workspace. Such a directory installs on its own.
