---
"pacquet": patch
---

`pnpm setup` now installs pnpm under the package name `pnpm`, the name `pnpm self-update` uses, so both leave the same shims in the global bin directory. After a self-update on Windows, PowerShell ran pnpm through `pnpm.cmd` and asked "Terminate batch job (Y/N)?" on Ctrl+C [#15567](https://github.com/pnpm/pnpm/issues/15567).
