---
"@pnpm/exec.commands": patch
"pnpm": patch
---

Shortened the `pnpm dlx` cache path so deep dependency trees no longer overflow Windows' `MAX_PATH`, which could make a dependency's lifecycle script fail with `spawn cmd.exe ENOENT`.
