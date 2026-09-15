---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

POSIX bin shims now convert a Windows-form path such as `C:\node_modules\.bin\tsc` correctly. The shim mangled the backslashes in such a path and could not reach the package it runs. Installing again replaces the shims already in `node_modules` [#14867](https://github.com/pnpm/pnpm/issues/14867).
