---
"@pnpm/bins.cmd-shim": patch
"pnpm": patch
"pacquet": patch
---

PowerShell command shims now run tools whose paths contain non-ASCII characters in Windows PowerShell 5.1 [#16217](https://github.com/pnpm/pnpm/issues/16217).
