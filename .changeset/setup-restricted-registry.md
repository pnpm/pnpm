---
"pacquet": patch
"pnpm": patch
"@pnpm/engine.pm.commands": patch
---

`pnpm setup` now completes when Windows registry environment values cannot be read or written. It prints PowerShell commands to configure `PNPM_HOME` and `PATH` without registry access [pnpm/pnpm#17167](https://github.com/pnpm/pnpm/issues/17167).
