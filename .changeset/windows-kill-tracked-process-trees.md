---
"@pnpm/exec.lifecycle": patch
"@pnpm/exec.commands": patch
"pnpm": patch
---

Fixed orphaned child processes on Windows when pnpm exits on an error while commands spawned by `pnpm exec` or `pnpm dlx` are still running (for example, when one project's command fails during `pnpm --recursive exec`). The PIDs of these commands are now recorded when they are spawned and their whole process trees are terminated with `taskkill` on an error exit. Previously the cleanup relied on enumerating the system process list, which is so slow on Windows that the enumeration hit its timeout and the cleanup was silently skipped [#12406](https://github.com/pnpm/pnpm/issues/12406).
