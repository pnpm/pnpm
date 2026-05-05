---
"@pnpm/config.reader": patch
"pnpm": patch
---

Accept `PNPM_CONFIG_*` (uppercase) environment variables in addition to `pnpm_config_*`. Previously, only the lowercase form was honored, so env vars renamed per the v11 migration guide (e.g. `PNPM_CONFIG_USERCONFIG`) silently had no effect on case-sensitive systems like macOS and Linux [#11465](https://github.com/pnpm/pnpm/issues/11465).
