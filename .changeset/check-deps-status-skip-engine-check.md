---
"@pnpm/deps.status": patch
"pnpm": patch
---

Skip the engine check when scanning workspace projects in `checkDepsStatus`. The dependency status check (run by `verifyDepsBeforeRun`) was calling `findWorkspaceProjects` without a `nodeVersion`, causing the engine check to fall back to the system Node from `PATH` and emit spurious "Unsupported engine" warnings before scripts ran. Engine validation still happens during install.
