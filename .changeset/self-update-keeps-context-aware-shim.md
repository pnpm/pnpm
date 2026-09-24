---
"pacquet": patch
---

`pnpm self-update` now keeps the `pnpm` command that `pnpm setup` installed as a context-aware shim and points it at the new version. It also stops writing `pnpm.cmd` and `pnpm.ps1` shims beside it. On Windows, those shims ran pnpm through a batch file, which asked "Terminate batch job (Y/N)?" on Ctrl+C [#15567](https://github.com/pnpm/pnpm/issues/15567).
