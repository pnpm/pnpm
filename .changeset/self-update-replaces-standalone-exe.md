---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

On Windows, `pnpm self-update` now replaces a `pnpm.exe` left in `PNPM_HOME` or in its `bin` directory. That executable ran before the updated `pnpm.cmd` shim, so `pnpm --version` kept printing the old version after a successful update [#9094](https://github.com/pnpm/pnpm/issues/9094).
