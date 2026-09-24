---
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
"pacquet": patch
---

Fixed scripts failing with `'an-compile' is not recognized` when `scriptShell` is set to `cmd.exe` on Windows [#7181](https://github.com/pnpm/pnpm/issues/7181). pnpm passed `-c` to every custom shell, and `cmd.exe` took the first `/c` inside the script (such as `node install/can-compile`) as its own switch. A `scriptShell` of `cmd` or `cmd.exe` now gets the same `/d /s /c` arguments and verbatim script as the default Windows shell.
