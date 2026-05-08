---
"@pnpm/config.reader": patch
"pnpm": patch
---

Honor `NPM_CONFIG_USERCONFIG` (and its lowercase `npm_config_userconfig` form) as a low-priority fallback when locating the user-level `.npmrc`. This restores compatibility with environments that point npm at a custom auth file via that env var — most notably `actions/setup-node`, which writes registry credentials to `${runner.temp}/.npmrc` and exports `NPM_CONFIG_USERCONFIG` to reference it. Without this, GitHub Actions workflows using `actions/setup-node` to authenticate to private registries broke after upgrading to pnpm v11. PNPM-prefixed env vars and `npmrcAuthFile` from the global `config.yaml` continue to take precedence [#11539](https://github.com/pnpm/pnpm/issues/11539).
