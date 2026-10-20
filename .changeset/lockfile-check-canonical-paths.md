---
"pacquet": patch
---

Reduced the CPU time that `pnpm install` spends checking whether the lockfile is up to date in workspaces whose projects depend on each other. On a workspace with 6,833 projects, the check uses about 1 second less CPU time.
