---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

On Windows, the shell shim of a `.cmd` or `.bat` bin now runs it under Git Bash when MSYS argument conversion is off, such as with `MSYS_NO_PATHCONV=1` or `MSYS2_ARG_CONV_EXCL='*'`. The shim used to start an interactive cmd.exe prompt instead [#17480](https://github.com/pnpm/pnpm/issues/17480).
