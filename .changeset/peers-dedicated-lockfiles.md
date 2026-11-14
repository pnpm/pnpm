---
"@pnpm/deps.inspection.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm peers` in workspaces with `sharedWorkspaceLockfile: false` now checks each selected project's `pnpm-lock.yaml`.
