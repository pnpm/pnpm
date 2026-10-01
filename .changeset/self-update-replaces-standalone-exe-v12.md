---
"pacquet": patch
---

On Windows, `pnpm self-update` now replaces a `pnpm.exe` left in `PNPM_HOME` or in `PNPM_HOME\bin`. Windows ran that executable in place of the updated `pnpm.cmd` shim, so `pnpm --version` kept printing the old version after a successful update. If the executable was in `PNPM_HOME`, `self-update` now asks you to run `pnpm setup` [#9094](https://github.com/pnpm/pnpm/issues/9094).
