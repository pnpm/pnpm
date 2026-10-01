---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

On Windows, `pnpm self-update` now replaces a `pnpm.exe` left in `PNPM_HOME` or in `PNPM_HOME\bin`. In `PNPM_HOME`, that executable kept running the old version after a successful update. In `PNPM_HOME\bin`, the update failed with `EPERM`. If the executable was in `PNPM_HOME`, `self-update` now asks you to run `pnpm setup` [#9094](https://github.com/pnpm/pnpm/issues/9094).
