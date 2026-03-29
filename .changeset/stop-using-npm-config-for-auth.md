---
"@pnpm/config.commands": minor
"@pnpm/config.reader": patch
"pnpm": minor
---

`pnpm config get/set/delete` no longer shells out to `npm config` for auth-related settings. Auth settings (registry, tokens, credentials, scoped registries) are now read from and written to the INI config files directly.

Auth settings from the pnpm global rc file (`~/.config/pnpm/rc`) now take priority over `~/.npmrc`, so tokens written by `pnpm login` are correctly picked up by `pnpm publish`.
