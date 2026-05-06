---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

Fixed the `pn`, `pnpx`, and `pnx` aliases failing in Git Bash / MSYS2 on Windows when pnpm was installed via `@pnpm/exe` (or after `pnpm self-update`) [#11486](https://github.com/pnpm/pnpm/issues/11486). Running `pnpx` (or `pnx`) printed the cmd.exe banner and dropped the user into an interactive command prompt instead of running `pnpm dlx`. The `bin` field rewrite on Windows was pointing those aliases at `.cmd` files; cmd-shim's Bash shim for a `.cmd` target wraps it in `exec cmd /C ...`, and MSYS2 mangles `/C` into a Windows path before cmd.exe sees it. The aliases are now `.exe` hardlinks of the SEA binary, which detects which name it was launched as via `process.execPath` and prepends `dlx` for `pnpx` / `pnx`.
