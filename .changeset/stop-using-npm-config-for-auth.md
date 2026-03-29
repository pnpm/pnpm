---
"@pnpm/config.commands": minor
"pnpm": minor
---

`pnpm config get/set/delete` no longer shells out to `npm config` for auth-related settings. Auth settings (registry, tokens, credentials, scoped registries) are now read from and written to the INI config files directly.
