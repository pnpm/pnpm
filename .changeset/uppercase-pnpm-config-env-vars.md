---
"@pnpm/config.reader": patch
"pnpm": patch
---

Accept `PNPM_CONFIG_*` (uppercase) environment variables in addition to `pnpm_config_*`. Previously, only the lowercase form was honored, which made env vars renamed per the v11 migration guide (e.g. `PNPM_CONFIG_USERCONFIG`) silently have no effect on case-sensitive systems like macOS and Linux. The setting that determines which user-level `.npmrc` is loaded (`userconfig` / `npmrc-auth-file`) can now also be supplied via env var [#11465](https://github.com/pnpm/pnpm/issues/11465).
