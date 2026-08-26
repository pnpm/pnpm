---
"pacquet": patch
---

Added support for the `globalPnpmfile` setting, which names a user-level pnpmfile that runs for every project ahead of the project's own. Like pnpm, it is left out of the lockfile's `pnpmfileChecksum`, so editing it does not decide whether a lockfile is still current. `pnpmfile` and `globalPnpmfile` are now also readable from `PNPM_CONFIG_PNPMFILE` and `PNPM_CONFIG_GLOBAL_PNPMFILE`.
