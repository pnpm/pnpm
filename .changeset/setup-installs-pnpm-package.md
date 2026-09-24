---
"pacquet": patch
---

`pnpm setup` now installs the CLI under the package name `pnpm`, the name `pnpm self-update` uses. On Windows, both commands now leave the same shims in the global bin directory, so `pnpm self-update` no longer changes how PowerShell launches pnpm. Before, a fresh install wrote a `pnpm.ps1` shim that the next self-update deleted, and PowerShell then ran pnpm through `pnpm.cmd`, which asked "Terminate batch job (Y/N)?" on Ctrl+C [#15567](https://github.com/pnpm/pnpm/issues/15567).
