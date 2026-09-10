---
"@pnpm/workspace.state": minor
"@pnpm/installing.commands": patch
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

A failed write of the workspace state file is now reported as a warning instead of failing the command. `pnpm run` previously exited with code 1 without starting the script. On Windows, the write retries a transient rename failure for up to one minute, and warns if the file is still locked [#14550](https://github.com/pnpm/pnpm/issues/14550).
