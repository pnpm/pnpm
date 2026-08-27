---
"@pnpm/workspace.task-scheduler": patch
"@pnpm/exec.commands": patch
"pnpm": patch
---

Published the workspace task graph and scheduler as `@pnpm/workspace.task-scheduler` so other workspace commands can use the same dependency-aware scheduling as recursive run and exec.
