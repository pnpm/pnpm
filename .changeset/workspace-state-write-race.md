---
"@pnpm/deps.status": patch
"@pnpm/workspace.state": patch
"pnpm": patch
"pacquet": patch
---

A failed write of the workspace state file is now reported as a warning instead of failing the command. On Windows, the write retries transient rename failures before falling back to a warning [pnpm/pnpm#14550](https://github.com/pnpm/pnpm/issues/14550).
