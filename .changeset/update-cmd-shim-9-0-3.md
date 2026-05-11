---
"@pnpm/bins.linker": patch
"pnpm": patch
---

Updated `@zkochan/cmd-shim` to 9.0.3. The sh shim it writes for `.cmd` / `.bat` targets now escapes the `/C` switch as `//C`, so it survives the path translation Git Bash applies when launching `cmd.exe`. Without this, a bare `/C` was rewritten to `C:\` before reaching cmd.exe — the switch was dropped, cmd started interactively, and the calling script saw the cmd banner instead of the wrapped command's output. Affects any cmd-shim-wrapped batch script invoked from Git Bash / MSYS / Cygwin on Windows. See [pnpm/cmd-shim#55](https://github.com/pnpm/cmd-shim/pull/55).
