---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fix `pnpm_config_npmrc_auth_file` and `pnpm_config_userconfig` env vars not actually loading the custom `.npmrc`. The env vars were parsed and assigned to the resolved config, but only after `loadNpmrcConfig` had already read the default `~/.npmrc` — so the custom file path was set but never read. The relevant env vars are now consulted before the user-level `.npmrc` is loaded [#11465](https://github.com/pnpm/pnpm/issues/11465).
