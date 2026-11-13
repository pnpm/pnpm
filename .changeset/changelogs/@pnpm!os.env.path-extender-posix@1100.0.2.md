## 1100.0.2

### Patch Changes

- `pnpm setup` and the install script now support PowerShell as the login shell on Linux and macOS. They add `PNPM_HOME` to `PATH` in `profile.ps1` under `$XDG_CONFIG_HOME/powershell` or `~/.config/powershell` [#18026](https://github.com/pnpm/pnpm/issues/18026).
