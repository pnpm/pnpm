---
"@pnpm/os.env.path-extender-posix": patch
"@pnpm/os.env.path-extender": patch
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm setup` and the install script now support PowerShell as the login shell on Linux and macOS. They add `PNPM_HOME` to `PATH` in `profile.ps1` under `$XDG_CONFIG_HOME/powershell` or `~/.config/powershell` [#18026](https://github.com/pnpm/pnpm/issues/18026).
