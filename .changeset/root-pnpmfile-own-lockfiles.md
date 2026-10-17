---
"pacquet": patch
---

With `sharedWorkspaceLockfile: false`, `pnpm install` now runs the workspace root's pnpmfile for every project, after the project's own pnpmfile. Before, it ran only the pnpmfile in the project's directory.
