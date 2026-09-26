---
"@pnpm/bins.cmd-shim": patch
"pnpm": patch
"pacquet": patch
---

Fixed PowerShell command shims failing to run tools whose paths contain non-ASCII characters in Windows PowerShell 5.1 [#16217](https://github.com/pnpm/pnpm/issues/16217).
